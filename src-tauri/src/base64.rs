//! Minimal standard base64 (RFC 4648, padded) used for small images on the wire.
//! Kept dependency-free so the build stays reproducible offline.
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0] as u32;
        let b = chunk.get(1).copied().unwrap_or(0) as u32;
        let c = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (a << 16) | (b << 8) | c;
        out.push(ALPHABET[(triple >> 18) as usize & 63] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { ALPHABET[(triple >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { ALPHABET[triple as usize & 63] as char } else { '=' });
    }
    out
}

/// Strict decoder: rejects anything that is not canonical padded base64.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    if text.len() % 4 != 0 { return None; }
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let bytes = text.as_bytes();
    for (index, chunk) in bytes.chunks(4).enumerate() {
        let last = index == bytes.len() / 4 - 1;
        let mut values = [0u32; 4];
        let mut padding = 0;
        for (position, byte) in chunk.iter().enumerate() {
            if *byte == b'=' {
                // Padding is only valid at the end of the string, in the last two slots.
                if !last || position < 2 || (position == 2 && chunk[3] != b'=') { return None; }
                padding += 1;
                values[position] = 0;
                continue;
            }
            if padding > 0 { return None; }
            values[position] = ALPHABET.iter().position(|candidate| candidate == byte)? as u32;
        }
        let triple = (values[0] << 18) | (values[1] << 12) | (values[2] << 6) | values[3];
        out.push((triple >> 16) as u8);
        if padding < 2 { out.push((triple >> 8) as u8); }
        if padding < 1 { out.push(triple as u8); }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_and_decodes_padded_chunks() {
        for sample in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar", &[0u8, 255, 128, 1, 2, 3][..]] {
            let text = encode(sample);
            assert_eq!(decode(&text).as_deref(), Some(sample));
        }
        assert_eq!(encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(encode(b"fo"), "Zm8=");
    }

    #[test]
    fn rejects_malformed_input() {
        for bad in ["a", "ab", "abc", "====", "Zm9vYmFy=", "Zm 9v", "Zm9v-", "Z===", "Zm8=="] {
            assert!(decode(bad).is_none(), "{bad} should not decode");
        }
    }
}
