//! TLS configuration for already verified identities only.
//! First-contact onboarding is deliberately not implemented here; untrusted peers fail closed.
//! A TLS client can finish its local handshake before the server has accepted its
//! certificate. Integration must require an authenticated application-level ready
//! message before transmitting chat content; these configs alone do not enforce it.
use std::{collections::BTreeSet, sync::Arc};
use rustls::{
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    server::danger::{ClientCertVerified, ClientCertVerifier},
    crypto::{ring::default_provider, verify_tls12_signature, verify_tls13_signature},
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime},
    CertificateError, ClientConfig, DigitallySignedStruct, DistinguishedName, Error, ServerConfig, SignatureScheme,
};
use crate::{certificate, identity::Identity};

type TlsResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug)]
struct PinnedKeys {
    fingerprints: BTreeSet<String>,
}

impl PinnedKeys {
    fn new(fingerprints: impl IntoIterator<Item = String>) -> Result<Self, Error> {
        let fingerprints: BTreeSet<String> = fingerprints.into_iter().collect();
        if fingerprints.is_empty() || fingerprints.len() > 1000 || fingerprints.iter().any(|value|
            value.len() != 64 || !value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))) {
            return Err(Error::General("invalid trusted fingerprints".into()));
        }
        Ok(Self { fingerprints })
    }

    fn verify(&self, certificate: &CertificateDer<'_>, intermediates: &[CertificateDer<'_>]) -> Result<(), Error> {
        if !intermediates.is_empty() { return Err(Error::InvalidCertificate(CertificateError::BadEncoding)); }
        let pin = certificate::fingerprint(certificate.as_ref()).map_err(|_| Error::InvalidCertificate(CertificateError::BadEncoding))?;
        if !self.fingerprints.contains(&pin) { return Err(Error::InvalidCertificate(CertificateError::UnknownIssuer)); }
        Ok(())
    }
}

// The pin only identifies the expected key. Rustls must additionally verify the
// TLS CertificateVerify handshake signature, proving possession of its private key.
impl ServerCertVerifier for PinnedKeys {
    fn verify_server_cert(&self, cert: &CertificateDer<'_>, intermediates: &[CertificateDer<'_>], _: &ServerName<'_>, _: &[u8], _: UnixTime) -> Result<ServerCertVerified, Error> {
        self.verify(cert, intermediates)?;
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, sig: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(message, cert, sig, &default_provider().signature_verification_algorithms)
    }
    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, sig: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(message, cert, sig, &default_provider().signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        default_provider().signature_verification_algorithms.supported_schemes()
    }
}

impl ClientCertVerifier for PinnedKeys {
    fn root_hint_subjects(&self) -> &[DistinguishedName] { &[] }
    fn verify_client_cert(&self, cert: &CertificateDer<'_>, intermediates: &[CertificateDer<'_>], _: UnixTime) -> Result<ClientCertVerified, Error> {
        self.verify(cert, intermediates)?;
        Ok(ClientCertVerified::assertion())
    }
    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, sig: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(message, cert, sig, &default_provider().signature_verification_algorithms)
    }
    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, sig: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(message, cert, sig, &default_provider().signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        default_provider().signature_verification_algorithms.supported_schemes()
    }
}

fn credentials(identity: &Identity) -> TlsResult<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    let cert = CertificateDer::from(identity.certificate_der()?);
    let key = PrivatePkcs8KeyDer::from(identity.private_key_der().to_vec());
    Ok((vec![cert], key.into()))
}

/// Pin the expected server key; the supplied fingerprint must already be user-verified.
pub fn client_config(identity: &Identity, verified_server: String) -> TlsResult<ClientConfig> {
    let pins = Arc::new(PinnedKeys::new([verified_server])?);
    let (cert, key) = credentials(identity)?;
    let mut config = ClientConfig::builder_with_provider(Arc::new(default_provider()))
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous().with_custom_certificate_verifier(pins)
        .with_client_auth_cert(cert, key)?;
    config.resumption = rustls::client::Resumption::disabled();
    Ok(config)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::{fs, io::{Cursor, Read, Write}, path::Path, sync::atomic::{AtomicU64, Ordering}, time::{SystemTime, UNIX_EPOCH}};
    use rustls::{ClientConnection, ServerConnection};
    static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(0);

    fn identity(label: &str) -> Identity {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("tls-tests");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{label}-{}-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(), NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed)));
        Identity::load_or_create(&path).unwrap()
    }

    fn handshake(client: ClientConfig, server: ServerConfig) -> Result<(ClientConnection, ServerConnection), String> {
        let name = ServerName::try_from("qbird.invalid").unwrap().to_owned();
        let mut c = ClientConnection::new(Arc::new(client), name).map_err(|e| e.to_string())?;
        let mut s = ServerConnection::new(Arc::new(server)).map_err(|e| e.to_string())?;
        for _ in 0..30 {
            let mut outbound = Vec::new();
            while c.wants_write() { c.write_tls(&mut outbound).map_err(|e| e.to_string())?; }
            if !outbound.is_empty() {
                s.read_tls(&mut Cursor::new(outbound)).map_err(|e| e.to_string())?;
                s.process_new_packets().map_err(|e| e.to_string())?;
            }
            let mut inbound = Vec::new();
            while s.wants_write() { s.write_tls(&mut inbound).map_err(|e| e.to_string())?; }
            if !inbound.is_empty() {
                c.read_tls(&mut Cursor::new(inbound)).map_err(|e| e.to_string())?;
                c.process_new_packets().map_err(|e| e.to_string())?;
            }
            if !c.is_handshaking() && !s.is_handshaking() { return Ok((c, s)); }
        }
        Err("handshake did not complete".into())
    }

    #[test]
    fn verified_peers_exchange_ciphertext_and_tampering_is_rejected() {
        let alice = identity("alice");
        let bob = identity("bob");
        let client = client_config(&alice, bob.fingerprint()).unwrap();
        let server = server_config(&bob, vec![alice.fingerprint()]).unwrap();
        let (mut c, mut s) = handshake(client, server).unwrap();
        assert_eq!(c.protocol_version(), Some(rustls::ProtocolVersion::TLSv1_3));
        let message = b"private test message 812761";
        c.writer().write_all(message).unwrap();
        let mut wire = Vec::new();
        c.write_tls(&mut wire).unwrap();
        assert!(!wire.windows(message.len()).any(|part| part == message));
        s.read_tls(&mut Cursor::new(&wire)).unwrap();
        s.process_new_packets().unwrap();
        let mut received = vec![0; message.len()];
        s.reader().read_exact(&mut received).unwrap();
        assert_eq!(received, message);
        c.writer().write_all(message).unwrap();
        wire.clear();
        c.write_tls(&mut wire).unwrap();
        let end = wire.len() - 1;
        wire[end] ^= 1;
        s.read_tls(&mut Cursor::new(wire)).unwrap();
        assert!(s.process_new_packets().is_err());
    }

    #[test]
    fn changed_or_unknown_identity_is_rejected_on_both_sides() {
        let alice = identity("alice");
        let bob = identity("bob");
        let impostor = identity("impostor");
        assert!(handshake(client_config(&alice, bob.fingerprint()).unwrap(), server_config(&impostor, vec![alice.fingerprint()]).unwrap()).is_err());
        assert!(handshake(client_config(&impostor, bob.fingerprint()).unwrap(), server_config(&bob, vec![alice.fingerprint()]).unwrap()).is_err());
        assert!(server_config(&bob, vec![]).is_err());
        assert!(client_config(&alice, "not-a-fingerprint".into()).is_err());
    }

    #[test]
    fn server_requires_a_client_certificate() {
        let alice = identity("alice");
        let bob = identity("bob");
        let pins = Arc::new(PinnedKeys::new([bob.fingerprint()]).unwrap());
        let no_certificate = ClientConfig::builder_with_provider(Arc::new(default_provider()))
            .with_protocol_versions(&[&rustls::version::TLS13]).unwrap()
            .dangerous().with_custom_certificate_verifier(pins).with_no_client_auth();
        assert!(handshake(no_certificate, server_config(&bob, vec![alice.fingerprint()]).unwrap()).is_err());
    }

    #[test]
    fn copied_certificate_with_wrong_private_key_is_rejected() {
        let alice = identity("alice");
        let bob = identity("bob");
        let attacker = identity("attacker");
        let copied_cert = vec![CertificateDer::from(bob.certificate_der().unwrap())];
        let attacker_key: PrivateKeyDer<'static> = PrivatePkcs8KeyDer::from(attacker.private_key_der().to_vec()).into();
        let signing_key = default_provider().key_provider.load_private_key(attacker_key).unwrap();
        // Deliberately bypass the normal key/certificate consistency check to simulate an attacker.
        let forged = rustls::sign::CertifiedKey::new(copied_cert, signing_key);
        let resolver = Arc::new(rustls::sign::SingleCertAndKey::from(forged));
        let pins = Arc::new(PinnedKeys::new([alice.fingerprint()]).unwrap());
        let fake_server = ServerConfig::builder_with_provider(Arc::new(default_provider()))
            .with_protocol_versions(&[&rustls::version::TLS13]).unwrap()
            .with_client_cert_verifier(pins).with_cert_resolver(resolver);
        let result = handshake(client_config(&alice, bob.fingerprint()).unwrap(), fake_server);
        assert!(result.is_err(), "the copied certificate must not prove private key possession");
    }
}

/// Require a client certificate whose key is already user-verified.
pub fn server_config(identity: &Identity, verified_clients: Vec<String>) -> TlsResult<ServerConfig> {
    let pins = Arc::new(PinnedKeys::new(verified_clients)?);
    let (cert, key) = credentials(identity)?;
    let mut config = ServerConfig::builder_with_provider(Arc::new(default_provider()))
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_client_cert_verifier(pins)
        .with_single_cert(cert, key)?;
    config.send_tls13_tickets = 0;
    Ok(config)
}
