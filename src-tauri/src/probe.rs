
//! Untrusted first-contact probe. It proves private-key possession but never grants
//! trust and never exposes the TLS stream for chat or other application data.
use std::{io::{self, Read, Write}, net::{SocketAddr, TcpStream}, sync::Arc, thread, time::{Duration, Instant}};
use rustls::{
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    server::danger::{ClientCertVerified, ClientCertVerifier},
    crypto::{ring::default_provider, verify_tls12_signature, verify_tls13_signature},
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime},
    CertificateError, ClientConfig, ClientConnection, DigitallySignedStruct, DistinguishedName, Error,
    ServerConfig, ServerConnection, SignatureScheme,
};
use crate::{address::is_unicast, certificate, identity::Identity};

const ALPN: &[u8] = b"qbird-probe/1";
const AUTO_ALPN: &[u8] = b"qbird-auto-probe/1";
/// One-way "I added you" notice so a single add can become mutual. The TLS client
/// certificate still proves the sender; the body only carries display hints.
const CONTACT_ALPN: &[u8] = b"qbird-contact/2";
const MAX_CONTACT_BYTES: usize = 2048;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ContactReply { Accepted = 1, Pending = 2, VerifyRequired = 3, Rejected = 4, Unavailable = 5 }

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContactAnnounce {
    #[serde(default)]
    pub nickname: String,
    #[serde(default)]
    pub username: Option<String>,
    /// `announce` (legacy mutual add), `request`, `accept` or `reject`.
    #[serde(default)]
    pub kind: Option<String>,
    /// Requester's `ip:port`, used to send the accept or reject back.
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub request_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProbeKind { Manual, Automatic, Contact }
const TIMEOUT: Duration = Duration::from_secs(5);
type ProbeResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug)]
struct ProbeVerifier;

impl ProbeVerifier {
    fn check(cert: &CertificateDer<'_>, intermediates: &[CertificateDer<'_>]) -> Result<(), Error> {
        if !intermediates.is_empty() { return Err(Error::InvalidCertificate(CertificateError::BadEncoding)); }
        certificate::fingerprint(cert.as_ref()).map_err(|_| Error::InvalidCertificate(CertificateError::BadEncoding))?;
        Ok(())
    }
}

impl ServerCertVerifier for ProbeVerifier {
    fn verify_server_cert(&self, cert: &CertificateDer<'_>, chain: &[CertificateDer<'_>], _: &ServerName<'_>, _: &[u8], _: UnixTime) -> Result<ServerCertVerified, Error> {
        Self::check(cert, chain)?; Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, sig: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(message, cert, sig, &default_provider().signature_verification_algorithms)
    }
    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, sig: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(message, cert, sig, &default_provider().signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> { default_provider().signature_verification_algorithms.supported_schemes() }
}

impl ClientCertVerifier for ProbeVerifier {
    fn root_hint_subjects(&self) -> &[DistinguishedName] { &[] }
    fn verify_client_cert(&self, cert: &CertificateDer<'_>, chain: &[CertificateDer<'_>], _: UnixTime) -> Result<ClientCertVerified, Error> {
        Self::check(cert, chain)?; Ok(ClientCertVerified::assertion())
    }
    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, sig: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(message, cert, sig, &default_provider().signature_verification_algorithms)
    }
    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, sig: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(message, cert, sig, &default_provider().signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> { default_provider().signature_verification_algorithms.supported_schemes() }
}

fn credentials(identity: &Identity) -> ProbeResult<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    let cert = CertificateDer::from(identity.certificate_der()?);
    let key = PrivatePkcs8KeyDer::from(identity.private_key_der().to_vec());
    Ok((vec![cert], key.into()))
}

fn client_config(identity: &Identity, automatic: bool) -> ProbeResult<ClientConfig> {
    let (cert, key) = credentials(identity)?;
    let mut config = ClientConfig::builder_with_provider(Arc::new(default_provider()))
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous().with_custom_certificate_verifier(Arc::new(ProbeVerifier))
        .with_client_auth_cert(cert, key)?;
    config.resumption = rustls::client::Resumption::disabled();
    config.alpn_protocols = vec![if automatic { AUTO_ALPN } else { ALPN }.to_vec()];
    Ok(config)
}

fn server_config(identity: &Identity) -> ProbeResult<ServerConfig> {
    let (cert, key) = credentials(identity)?;
    let mut config = ServerConfig::builder_with_provider(Arc::new(default_provider()))
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_client_cert_verifier(Arc::new(ProbeVerifier)).with_single_cert(cert, key)?;
    config.alpn_protocols = vec![ALPN.to_vec(), AUTO_ALPN.to_vec(), CONTACT_ALPN.to_vec()];
    config.send_tls13_tickets = 0;
    Ok(config)
}

fn fingerprint(chain: Option<&[CertificateDer<'_>]>, alpn: Option<&[u8]>) -> ProbeResult<String> {
    if alpn != Some(ALPN) && alpn != Some(AUTO_ALPN) && alpn != Some(CONTACT_ALPN) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "unexpected probe protocol").into());
    }
    let chain = chain.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "peer certificate missing"))?;
    if chain.len() != 1 { return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid peer certificate chain").into()); }
    certificate::fingerprint(chain[0].as_ref()).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e).into())
}

fn prepare(stream: &TcpStream) -> io::Result<()> {
    stream.set_nonblocking(true)
}

fn handshake(connection: &mut rustls::Connection, stream: &mut TcpStream) -> io::Result<()> {
    let deadline = Instant::now() + TIMEOUT;
    let mut received = 0usize;
    let mut record_remaining = 0usize;
    loop {
        if Instant::now() >= deadline { return Err(io::Error::new(io::ErrorKind::TimedOut, "probe handshake timed out")); }
        if !connection.is_handshaking() && !connection.wants_write() { return Ok(()); }
        let mut progressed = false;
        if connection.wants_write() {
            match connection.write_tls(stream) {
                Ok(0) => return Err(io::Error::new(io::ErrorKind::WriteZero, "probe connection closed")),
                Ok(_) => progressed = true,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {},
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {},
                Err(e) => return Err(e),
            }
        }
        if connection.is_handshaking() && connection.wants_read() {
            // A contact notice follows the TLS handshake on the same socket. Do not
            // consume bytes from the next protocol while reading the last TLS record.
            if record_remaining == 0 {
                let mut header = [0u8; 5];
                match stream.peek(&mut header) {
                    Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "probe connection closed")),
                    Ok(n) if n < header.len() => {},
                    Ok(_) => record_remaining = header.len() + u16::from_be_bytes([header[3], header[4]]) as usize,
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::Interrupted => {},
                    Err(e) => return Err(e),
                }
            }
            if record_remaining == 0 { if !progressed { thread::sleep(Duration::from_millis(5)); } continue; }
            match connection.read_tls(&mut (&mut *stream).take(record_remaining as u64)) {
                Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "probe connection closed")),
                Ok(n) => {
                    record_remaining -= n;
                    received += n;
                    if received > 256 * 1024 { return Err(io::Error::new(io::ErrorKind::InvalidData, "probe handshake too large")); }
                    let state = connection.process_new_packets().map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                    if state.plaintext_bytes_to_read() != 0 { return Err(io::Error::new(io::ErrorKind::InvalidData, "probe forbids application data")); }
                    progressed = true;
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {},
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {},
                Err(e) => return Err(e),
            }
        }
        if !progressed { thread::sleep(Duration::from_millis(5)); }
    }
}

/// Returns an untrusted fingerprint for display only; it does not confirm a contact.
pub fn connect(identity: &Identity, address: SocketAddr) -> ProbeResult<String> {
    connect_inner(identity, address, false, TIMEOUT)
}

pub(crate) fn connect_auto(identity: &Identity, address: SocketAddr) -> ProbeResult<String> {
    connect_inner(identity, address, true, TIMEOUT)
}

/// Range scanning uses a shorter connect timeout so a /24 stays bounded.
pub(crate) fn connect_with_timeout(identity: &Identity, address: SocketAddr, timeout: Duration) -> ProbeResult<String> {
    connect_inner(identity, address, false, timeout)
}

fn connect_inner(identity: &Identity, address: SocketAddr, automatic: bool, timeout: Duration) -> ProbeResult<String> {
    if address.port() == 0 || !is_unicast(address.ip()) { return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid address").into()); }
    let mut stream = TcpStream::connect_timeout(&address, timeout)?;
    prepare(&stream)?;
    let mut connection = rustls::Connection::Client(ClientConnection::new(Arc::new(client_config(identity, automatic)?), ServerName::try_from("qbird.invalid")?.to_owned())?);
    handshake(&mut connection, &mut stream)?;
    fingerprint(connection.peer_certificates(), connection.alpn_protocol())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::{fs, io::Write, net::TcpListener, path::Path, thread, time::{SystemTime, UNIX_EPOCH}};

    fn identity(label: &str) -> Identity {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("probe-tests");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{label}-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        Identity::load_or_create(&path).unwrap()
    }

    #[test]
    fn both_sides_observe_proven_but_untrusted_fingerprints_over_tcp() {
        let client = identity("client");
        let server = identity("server");
        let expected_client = client.fingerprint();
        let expected_server = server.fingerprint();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let worker = thread::spawn(move || accept(&server, listener.accept().unwrap().0).unwrap());
        assert_eq!(connect(&client, addr).unwrap(), expected_server);
        assert_eq!(worker.join().unwrap(), expected_client);
    }

    #[test]
    fn plaintext_and_invalid_addresses_are_rejected() {
        let server = identity("server");
        let client = identity("client");
        assert!(connect(&client, "0.0.0.0:1".parse().unwrap()).is_err());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let worker = thread::spawn(move || accept(&server, listener.accept().unwrap().0).is_err());
        let mut stream = TcpStream::connect(addr).unwrap();
        stream.write_all(b"plain text is not TLS").unwrap();
        drop(stream);
        assert!(worker.join().unwrap());
    }
}

/// Handles one probe only; the caller must enforce a bounded number of concurrent probes.
pub fn accept(identity: &Identity, mut stream: TcpStream) -> ProbeResult<String> {
    accept_classified(identity, &mut stream).map(|(fingerprint, _, _)| fingerprint)
}

pub(crate) fn accept_classified(identity: &Identity, stream: &mut TcpStream) -> ProbeResult<(String, ProbeKind, rustls::Connection)> {
    prepare(stream)?;
    let mut connection = rustls::Connection::Server(ServerConnection::new(Arc::new(server_config(identity)?))?);
    handshake(&mut connection, stream)?;
    let fingerprint = fingerprint(connection.peer_certificates(), connection.alpn_protocol())?;
    let kind = match connection.alpn_protocol() {
        Some(AUTO_ALPN) => ProbeKind::Automatic,
        Some(CONTACT_ALPN) => ProbeKind::Contact,
        _ => ProbeKind::Manual,
    };
    Ok((fingerprint, kind, connection))
}

/// Reads the bounded JSON body of a contact announcement. The caller must close the
/// connection immediately afterwards; no chat payload is accepted on this ALPN.
pub(crate) fn read_contact_announce(connection: &mut rustls::Connection, stream: &mut TcpStream) -> ProbeResult<ContactAnnounce> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    let rustls::Connection::Server(connection) = connection else { return Err(io::Error::new(io::ErrorKind::InvalidInput, "expected server connection").into()) };
    let mut stream = rustls::Stream::new(connection, stream);
    let mut header = [0u8; 3];
    stream.read_exact(&mut header)?;
    if header[0] != 1 { return Err(io::Error::new(io::ErrorKind::InvalidData, "unsupported contact version").into()); }
    let length = u16::from_be_bytes([header[1], header[2]]) as usize;
    if length == 0 || length > MAX_CONTACT_BYTES { return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid contact size").into()); }
    let mut body = vec![0u8; length];
    stream.read_exact(&mut body)?;
    let announce: ContactAnnounce = serde_json::from_slice(&body)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid contact body"))?;
    if announce.nickname.chars().count() > 64 || announce.nickname.chars().any(char::is_control) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid nickname").into());
    }
    if let Some(kind) = announce.kind.as_deref() {
        if !matches!(kind, "announce" | "request" | "accept" | "reject" | "policy" | "status") {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid contact kind").into());
        }
    }
    if let Some(address) = announce.address.as_deref() {
        if address.is_empty() || address.len() > 128 || crate::address::parse_manual_address(address).is_err() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid contact address").into());
        }
    }
    if announce.reason.as_deref().is_some_and(|value| value.len() > 600 || value.chars().any(char::is_control)) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid contact reason").into());
    }
    if announce.request_id.as_deref().is_some_and(|value| value.len() != 32 || !value.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid request id").into());
    }
    if let Some(username) = announce.username.as_deref() {
        if username.len() > 20 || !username.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid username").into());
        }
    }
    Ok(announce)
}

/// Sends one acknowledgement byte; `true` means the peer stored the contact.
pub(crate) fn write_contact_ack(connection: &mut rustls::Connection, stream: &mut TcpStream, reply: ContactReply) -> ProbeResult<()> {
    stream.set_nonblocking(false)?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    let rustls::Connection::Server(connection) = connection else { return Err(io::Error::new(io::ErrorKind::InvalidInput, "expected server connection").into()) };
    let mut stream = rustls::Stream::new(connection, stream);
    stream.write_all(&[reply as u8])?;
    stream.flush()?;
    Ok(())
}

/// Tells a peer that we added it, so it can add us back without asking the user.
/// The peer is offline or has automatic contact acceptance disabled: this is advisory.
pub fn announce_contact(identity: &Identity, address: SocketAddr, announce: &ContactAnnounce) -> ProbeResult<bool> {
    Ok(announce_contact_inner(identity, address, announce, None)? == ContactReply::Accepted)
}

pub fn announce_contact_pinned(identity: &Identity, address: SocketAddr, announce: &ContactAnnounce, expected: &str) -> ProbeResult<bool> {
    Ok(announce_contact_inner(identity, address, announce, Some(expected))? == ContactReply::Accepted)
}

pub fn contact_exchange(identity: &Identity, address: SocketAddr, announce: &ContactAnnounce, expected: &str) -> ProbeResult<ContactReply> {
    announce_contact_inner(identity, address, announce, Some(expected))
}

fn announce_contact_inner(identity: &Identity, address: SocketAddr, announce: &ContactAnnounce, expected: Option<&str>) -> ProbeResult<ContactReply> {
    if address.port() == 0 || !is_unicast(address.ip()) { return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid address").into()); }
    let body = serde_json::to_vec(announce).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "contact body"))?;
    if body.is_empty() || body.len() > MAX_CONTACT_BYTES { return Err(io::Error::new(io::ErrorKind::InvalidInput, "contact body too large").into()); }
    let mut stream = TcpStream::connect_timeout(&address, TIMEOUT)?;
    prepare(&stream)?;
    let (cert, key) = credentials(identity)?;
    let mut config = ClientConfig::builder_with_provider(Arc::new(default_provider()))
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous().with_custom_certificate_verifier(Arc::new(ProbeVerifier))
        .with_client_auth_cert(cert, key)?;
    config.resumption = rustls::client::Resumption::disabled();
    config.alpn_protocols = vec![CONTACT_ALPN.to_vec()];
    let mut connection = rustls::Connection::Client(ClientConnection::new(Arc::new(config), ServerName::try_from("qbird.invalid")?.to_owned())?);
    handshake(&mut connection, &mut stream)?;
    if connection.alpn_protocol() != Some(CONTACT_ALPN) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "peer does not accept contact notices").into());
    }
    if let Some(expected) = expected {
        if fingerprint(connection.peer_certificates(), connection.alpn_protocol())? != expected {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "contact identity changed").into());
        }
    }
    stream.set_nonblocking(false)?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    let rustls::Connection::Client(ref mut client) = connection else { unreachable!() };
    let mut stream = rustls::Stream::new(client, &mut stream);
    let length = u16::try_from(body.len()).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "contact body too large"))?;
    stream.write_all(&[1])?;
    stream.write_all(&length.to_be_bytes())?;
    stream.write_all(&body)?;
    stream.flush()?;
    let mut ack = [0u8; 1];
    stream.read_exact(&mut ack)?;
    match ack[0] {
        1 => Ok(ContactReply::Accepted), 2 => Ok(ContactReply::Pending),
        3 => Ok(ContactReply::VerifyRequired), 4 => Ok(ContactReply::Rejected),
        5 => Ok(ContactReply::Unavailable),
        _ => Err(io::Error::new(io::ErrorKind::InvalidData, "invalid contact reply").into()),
    }
}