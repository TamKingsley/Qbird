use sha2::{Digest, Sha256};
use x509_parser::parse_x509_certificate;

/// SHA-256 of the DER SubjectPublicKeyInfo, not a hash of the renewable certificate.
pub fn fingerprint(certificate_der: &[u8]) -> Result<String, &'static str> {
    if certificate_der.is_empty() || certificate_der.len() > 16 * 1024 {
        return Err("invalid certificate size");
    }
    let (remaining, certificate) = parse_x509_certificate(certificate_der).map_err(|_| "invalid certificate")?;
    if !remaining.is_empty() { return Err("trailing certificate data"); }
    Ok(Sha256::digest(certificate.public_key().raw).iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::identity::Identity;
    use std::{fs, path::Path, time::{SystemTime, UNIX_EPOCH}};

    #[test]
    fn certificate_and_persistent_identity_have_identical_fingerprints() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("certificate-tests");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        let identity = Identity::load_or_create(&path).unwrap();
        let der = identity.certificate_der().unwrap();
        assert_eq!(fingerprint(&der).unwrap(), identity.fingerprint());
        let mut trailing = der;
        trailing.push(0);
        assert!(fingerprint(&trailing).is_err());
        assert!(fingerprint(b"not a certificate").is_err());
        assert!(fingerprint(&vec![0; 16 * 1024 + 1]).is_err());
    }
}
