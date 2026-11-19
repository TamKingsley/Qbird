//! Persistent identity. The private key is bound to the current Windows account
//! by DPAPI; this does not encrypt the chat database.
use std::{fs::OpenOptions, io::{self, Read, Write}, path::Path};
use rcgen::{CertificateParams, ExtendedKeyUsagePurpose, KeyPair, PublicKeyData};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

const MAGIC: &[u8] = b"QBIRD-IDENTITY-1\0";
const MAX_FILE_SIZE: u64 = 64 * 1024;

type IdentityResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub struct Identity {
    key: KeyPair,
}

impl Identity {
    /// Never replaces an existing file: unreadable/corrupt identities are errors.
    pub fn load_or_create(path: &Path) -> IdentityResult<Self> {
        match OpenOptions::new().read(true).open(path) {
            Ok(file) => {
                if file.metadata()?.len() > MAX_FILE_SIZE {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "identity file too large").into());
                }
                let mut data = Vec::new();
                file.take(MAX_FILE_SIZE + 1).read_to_end(&mut data)?;
                if data.len() as u64 > MAX_FILE_SIZE {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "identity file too large").into());
                }
                let protected = data.strip_prefix(MAGIC).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid identity file"))?;
                let secret = unprotect(protected)?;
                Ok(Self { key: KeyPair::try_from(secret.as_slice())? })
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let key = KeyPair::generate()?;
                let protected = protect(key.serialized_der())?;
                let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
                file.write_all(MAGIC)?;
                file.write_all(&protected)?;
                file.sync_all()?;
                Ok(Self { key })
            }
            Err(error) => Err(error.into()),
        }
    }

    /// SHA-256 of the full DER SubjectPublicKeyInfo, lowercase hexadecimal.
    pub fn fingerprint(&self) -> String {
        Sha256::digest(self.key.subject_public_key_info()).iter().map(|b| format!("{b:02x}")).collect()
    }

    /// The certificate may be renewed; the identity fingerprint is tied to its key.
    pub fn certificate_der(&self) -> IdentityResult<Vec<u8>> {
        let mut params = CertificateParams::new(Vec::<String>::new())?;
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth, ExtendedKeyUsagePurpose::ClientAuth];
        Ok(params.self_signed(&self.key)?.der().to_vec())
    }

    pub fn private_key_der(&self) -> Zeroizing<Vec<u8>> {
        Zeroizing::new(self.key.serialize_der())
    }
}

#[cfg(windows)]
fn protect(input: &[u8]) -> io::Result<Zeroizing<Vec<u8>>> {
    dpapi(input, true)
}

#[cfg(windows)]
fn unprotect(input: &[u8]) -> io::Result<Zeroizing<Vec<u8>>> {
    dpapi(input, false)
}

#[cfg(windows)]
fn dpapi(input: &[u8], encrypt: bool) -> io::Result<Zeroizing<Vec<u8>>> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::{Foundation::LocalFree, Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN}};
    let length = u32::try_from(input.len()).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "input too large"))?;
    let source = CRYPT_INTEGER_BLOB { cbData: length, pbData: input.as_ptr() as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let ok = unsafe {
        if encrypt {
            CryptProtectData(&source, null(), null(), null(), null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output)
        } else {
            CryptUnprotectData(&source, null_mut(), null(), null(), null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output)
        }
    };
    if ok == 0 { return Err(io::Error::last_os_error()); }
    let result = unsafe { Zeroizing::new(std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec()) };
    unsafe {
        // The DPAPI output for decryption contains plaintext key material.
        std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize).zeroize();
        LocalFree(output.pbData.cast());
    }
    Ok(result)
}

#[cfg(not(windows))]
fn protect(_: &[u8]) -> io::Result<Zeroizing<Vec<u8>>> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "identity protection is only implemented for Windows"))
}

#[cfg(not(windows))]
fn unprotect(_: &[u8]) -> io::Result<Zeroizing<Vec<u8>>> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "identity protection is only implemented for Windows"))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf, sync::atomic::{AtomicU64, Ordering}, time::{SystemTime, UNIX_EPOCH}};

    fn test_path(label: &str) -> PathBuf {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("identity-tests");
        fs::create_dir_all(&dir).unwrap();
        dir.join(format!("{}-{}-{}-{}", label, std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(), SEQUENCE.fetch_add(1, Ordering::Relaxed)))
    }

    #[test]
    fn identity_survives_reload_and_private_key_is_not_plaintext_on_disk() {
        let path = test_path("persist");
        let first = Identity::load_or_create(&path).unwrap();
        let fingerprint = first.fingerprint();
        let secret = first.private_key_der();
        let disk = fs::read(&path).unwrap();
        assert_eq!(fingerprint.len(), 64);
        assert!(!disk.windows(secret.len()).any(|bytes| bytes == secret.as_slice()));
        assert!(!first.certificate_der().unwrap().is_empty());
        let second = Identity::load_or_create(&path).unwrap();
        assert_eq!(second.fingerprint(), fingerprint);
        assert_eq!(second.private_key_der().as_slice(), secret.as_slice());
    }

    #[test]
    fn damaged_existing_identity_is_rejected_without_replacement() {
        let path = test_path("damaged");
        Identity::load_or_create(&path).unwrap();
        let mut bytes = fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        fs::write(&path, &bytes).unwrap();
        assert!(Identity::load_or_create(&path).is_err());
        assert_eq!(fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn unrelated_or_oversized_files_are_not_replaced() {
        let path = test_path("invalid");
        let bytes = vec![42; MAX_FILE_SIZE as usize + 1];
        fs::write(&path, &bytes).unwrap();
        assert!(Identity::load_or_create(&path).is_err());
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}
