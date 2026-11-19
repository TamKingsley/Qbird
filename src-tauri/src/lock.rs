//! Optional application lock.
//!
//! The lock password is stored as PBKDF2-HMAC-SHA256 with a random per-profile
//! salt; the plaintext password never touches disk. The lock itself is an
//! in-memory UI state: it hides the interface but does not encrypt the local
//! database. Passwords have a bounded length and failures are reported to the UI,
//! which is responsible for throttling repeated attempts.
use std::num::NonZeroU32;

use ring::{pbkdf2, rand::{SecureRandom, SystemRandom}};

use crate::storage::Store;

pub const MIN_PASSWORD: usize = 6;
pub const MAX_PASSWORD: usize = 128;
pub const MAX_IDLE_MINUTES: u32 = 240;

const ITERATIONS: u32 = 200_000;
const SALT_LEN: usize = 32;
const KEY_LEN: usize = 32;
const SALT_KEY: &str = "lock_salt";
const HASH_KEY: &str = "lock_hash";
const IDLE_KEY: &str = "lock_idle_minutes";

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

fn unhex(value: &str) -> Option<Vec<u8>> {
    if value.len() % 2 != 0 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    (0..value.len()).step_by(2).map(|index| u8::from_str_radix(&value[index..index + 2], 16).ok()).collect()
}

fn derive(password: &str, salt: &[u8]) -> [u8; KEY_LEN] {
    let rounds = NonZeroU32::new(ITERATIONS).expect("fixed iteration count");
    let mut key = [0u8; KEY_LEN];
    pbkdf2::derive(pbkdf2::PBKDF2_HMAC_SHA256, rounds, salt, password.as_bytes(), &mut key);
    key
}

/// Returns whether a password is configured. A half-written record counts as
/// not configured and never grants access.
pub fn is_configured(store: &Store) -> Result<bool, String> {
    Ok(load_record(store)?.is_some())
}

fn load_record(store: &Store) -> Result<Option<(Vec<u8>, Vec<u8>)>, String> {
    let salt = store.get_setting(SALT_KEY).map_err(|_| "无法读取应用锁设置".to_owned())?;
    let hash = store.get_setting(HASH_KEY).map_err(|_| "无法读取应用锁设置".to_owned())?;
    let (Some(salt), Some(hash)) = (salt, hash) else { return Ok(None); };
    let (Some(salt), Some(hash)) = (unhex(&salt), unhex(&hash)) else { return Ok(None); };
    if salt.len() != SALT_LEN || hash.len() != KEY_LEN { return Ok(None); }
    Ok(Some((salt, hash)))
}

pub fn validate_password(password: &str) -> Result<(), String> {
    let length = password.chars().count();
    if length < MIN_PASSWORD || length > MAX_PASSWORD {
        return Err(format!("应用锁密码需为 {MIN_PASSWORD}–{MAX_PASSWORD} 个字符"));
    }
    Ok(())
}

/// Constant-time verification. A wrong password and a missing lock both return false.
pub fn verify(store: &Store, password: &str) -> Result<bool, String> {
    let Some((salt, expected)) = load_record(store)? else { return Ok(false); };
    let actual = derive(password, &salt);
    // Lengths are fixed at KEY_LEN; compare every byte without early exit.
    let mut difference = 0u8;
    for (left, right) in actual.iter().zip(expected.iter()) { difference |= left ^ right; }
    Ok(difference == 0)
}

pub fn set_password(store: &Store, password: &str) -> Result<(), String> {
    validate_password(password)?;
    let mut salt = [0u8; SALT_LEN];
    SystemRandom::new().fill(&mut salt).map_err(|_| "无法生成随机盐值".to_owned())?;
    let hash = derive(password, &salt);
    store.set_setting(SALT_KEY, Some(&hex(&salt))).map_err(|_| "无法保存应用锁".to_owned())?;
    store.set_setting(HASH_KEY, Some(&hex(&hash))).map_err(|_| "无法保存应用锁".to_owned())?;
    Ok(())
}

pub fn remove_password(store: &Store) -> Result<(), String> {
    store.set_setting(SALT_KEY, None).map_err(|_| "无法移除应用锁".to_owned())?;
    store.set_setting(HASH_KEY, None).map_err(|_| "无法移除应用锁".to_owned())?;
    Ok(())
}

pub fn idle_minutes(store: &Store) -> Result<u32, String> {
    let value = store.get_setting(IDLE_KEY).map_err(|_| "无法读取空闲锁定设置".to_owned())?;
    Ok(value.and_then(|text| text.parse::<u32>().ok()).filter(|minutes| *minutes <= MAX_IDLE_MINUTES).unwrap_or(0))
}

/// `0` disables automatic locking; values above the cap are rejected.
pub fn set_idle_minutes(store: &Store, minutes: u32) -> Result<bool, String> {
    if minutes > MAX_IDLE_MINUTES { return Ok(false); }
    store.set_setting(IDLE_KEY, Some(&minutes.to_string())).map_err(|_| "无法保存空闲锁定设置".to_owned())?;
    Ok(true)
}

/// Seconds since the last system-wide keyboard or mouse input.
///
/// Returns `None` when the platform cannot report it; callers then fall back to
/// their own activity tracking.
#[cfg(windows)]
pub fn system_idle_seconds() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    // SAFETY: `info` is a live, correctly sized LASTINPUTINFO; the call only writes into it.
    if unsafe { GetLastInputInfo(&mut info) } == 0 {
        return None;
    }
    // Both values are 32-bit tick counts, so a wrapping subtraction is the documented way to
    // measure the delta across tick rollover.
    // SAFETY: GetTickCount has no preconditions and only reads a monotonic tick counter.
    let elapsed_ms = unsafe { GetTickCount() }.wrapping_sub(info.dwTime);
    Some(u64::from(elapsed_ms / 1_000))
}

#[cfg(not(windows))]
pub fn system_idle_seconds() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (Store, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "qbird-lock-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("qbird.sqlite3")).unwrap();
        (store, dir)
    }

    #[test]
    fn password_is_hashed_verified_and_removed() {
        let (store, dir) = temp_store();
        assert!(!is_configured(&store).unwrap());
        assert!(!verify(&store, "secret123").unwrap());
        assert!(set_password(&store, "short").is_err());
        assert!(set_password(&store, &"x".repeat(MAX_PASSWORD + 1)).is_err());
        set_password(&store, "secret123").unwrap();
        assert!(is_configured(&store).unwrap());
        assert!(verify(&store, "secret123").unwrap());
        assert!(!verify(&store, "secret124").unwrap());
        // The plaintext never appears in the settings table.
        let stored = store.get_setting(HASH_KEY).unwrap().unwrap();
        assert!(!stored.contains("secret"));
        assert_eq!(stored.len(), KEY_LEN * 2);
        remove_password(&store).unwrap();
        assert!(!is_configured(&store).unwrap());
        assert!(!verify(&store, "secret123").unwrap());
        drop(store);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn idle_minutes_are_bounded_and_persisted() {
        let (store, dir) = temp_store();
        assert_eq!(idle_minutes(&store).unwrap(), 0);
        assert!(!set_idle_minutes(&store, MAX_IDLE_MINUTES + 1).unwrap());
        assert!(set_idle_minutes(&store, 5).unwrap());
        assert_eq!(idle_minutes(&store).unwrap(), 5);
        assert!(set_idle_minutes(&store, 0).unwrap());
        assert_eq!(idle_minutes(&store).unwrap(), 0);
        drop(store);
        let _ = std::fs::remove_dir_all(dir);
    }
}
