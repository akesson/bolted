//! Minimal blocking HTTP/1.1 + TLS plumbing shared by the test server ([`super::server`]) and the
//! socket mock ([`super::netmock`]). This is deliberately *not* a general HTTP implementation: it
//! speaks exactly the subset the eleven §7 rules need, over `Connection: close` framing (one
//! exchange per socket), so the harness keeps full control of stall / truncate / redirect / cert
//! behaviour that a real HTTP library would hide.
//!
//! No async runtime: everything is blocking `std::net`, driven from std threads. Deadline and
//! cancellation are enforced by a watchdog that shuts the socket down (see [`super::netmock`]),
//! never by mid-record read timeouts (which would corrupt a TLS stream).

use std::io::{self, Read, Write};
use std::sync::Arc;
use std::sync::Once;

use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ClientConfig, ServerConfig};
use sha2::{Digest, Sha256};

/// A blocking bidirectional transport: either a plain `TcpStream` or a rustls `StreamOwned`.
pub trait ReadWrite: Read + Write + Send {}
impl<T: Read + Write + Send> ReadWrite for T {}

/// Install the `ring` crypto provider as rustls' process default. Idempotent; safe to call from
/// every server/adapter constructor.
pub fn install_crypto_provider() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // A second install would only happen if a host also installed one; ignore the result.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// A self-signed cert + key, plus the SHA-256 of its SubjectPublicKeyInfo (the [`crate::SpkiPin`]
/// value a pinning caller would carry).
pub struct GeneratedCert {
    /// The certificate, DER-encoded.
    pub cert_der: CertificateDer<'static>,
    /// The private key, PKCS#8 DER.
    pub key_der: PrivateKeyDer<'static>,
    /// SHA-256 of the certificate's SubjectPublicKeyInfo (the honest SPKI pin).
    pub spki_sha256: [u8; 32],
}

/// Why a wire-level setup step failed (cert generation, DER parsing). Mapped to a typed
/// [`crate::HttpError`] by the caller where it surfaces as a request outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireError {
    /// rcgen could not generate the certificate.
    CertGen,
    /// The certificate DER could not be parsed for its SPKI.
    SpkiParse,
}

/// Generate a self-signed cert for `sans`, returning it with its SPKI pin.
pub fn generate_cert(sans: Vec<String>) -> Result<GeneratedCert, WireError> {
    let ck = rcgen::generate_simple_self_signed(sans).map_err(|_| WireError::CertGen)?;
    let cert_der = ck.cert.der().clone();
    let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(ck.signing_key.serialize_der()));
    let spki_sha256 = spki_sha256(cert_der.as_ref())?;
    Ok(GeneratedCert {
        cert_der,
        key_der,
        spki_sha256,
    })
}

/// SHA-256 of the SubjectPublicKeyInfo carried in a DER-encoded X.509 certificate. This is the
/// honest SPKI pin: both the server (building the expected pin) and the mock's verifier (checking
/// the wire cert) compute it the same way.
pub fn spki_sha256(cert_der: &[u8]) -> Result<[u8; 32], WireError> {
    let (_, cert) =
        x509_parser::parse_x509_certificate(cert_der).map_err(|_| WireError::SpkiParse)?;
    let spki_der = cert.public_key().raw;
    let mut hasher = Sha256::new();
    hasher.update(spki_der);
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    Ok(out)
}

/// A rustls server config presenting `cert`/`key` with no client-auth. Panics are avoided: a bad
/// key is surfaced as `None`.
pub fn server_config(
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
) -> Option<Arc<ServerConfig>> {
    install_crypto_provider();
    ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .ok()
        .map(Arc::new)
}

/// A rustls client config using `verifier` as its (dangerous) server-cert verifier. The verifier
/// is where pinning and trust decisions live (see [`super::netmock`]).
pub fn client_config(
    verifier: Arc<dyn rustls::client::danger::ServerCertVerifier>,
) -> Arc<ClientConfig> {
    install_crypto_provider();
    let cfg = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    Arc::new(cfg)
}

/// Read from `r` until the end of the HTTP head (`\r\n\r\n`). Returns `(head, leftover)` where
/// `leftover` is any body bytes already read past the head. Bounded to guard a hostile peer.
pub fn read_head<R: Read + ?Sized>(r: &mut R) -> io::Result<(Vec<u8>, Vec<u8>)> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        if let Some(pos) = find_headers_end(&buf) {
            let leftover = buf.split_off(pos);
            return Ok((buf, leftover));
        }
        let n = r.read(&mut chunk)?;
        if n == 0 {
            // EOF before a complete head.
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > 64 * 1024 {
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }
    }
}

/// Index one past the `\r\n\r\n` that ends the head, if present.
fn find_headers_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

/// Read exactly `len` more body bytes, given `already` read past the head. Returns the full body.
/// A short read (peer closed early) is an `UnexpectedEof` — the truncation signal for rule 8.
pub fn read_body_exact<R: Read + ?Sized>(
    r: &mut R,
    already: Vec<u8>,
    len: usize,
) -> io::Result<Vec<u8>> {
    let mut body = already;
    body.truncate(len.min(body.len()));
    let mut got = body.len();
    let mut chunk = [0u8; 4096];
    while got < len {
        let n = r.read(&mut chunk)?;
        if n == 0 {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
        }
        let take = n.min(len - got);
        body.extend_from_slice(&chunk[..take]);
        got += take;
    }
    Ok(body)
}

/// Read a body of unknown length until EOF (`Connection: close`, no `Content-Length`).
pub fn read_body_to_end<R: Read + ?Sized>(r: &mut R, already: Vec<u8>) -> io::Result<Vec<u8>> {
    let mut body = already;
    let mut chunk = [0u8; 4096];
    loop {
        match r.read(&mut chunk) {
            Ok(0) => return Ok(body),
            Ok(n) => body.extend_from_slice(&chunk[..n]),
            Err(e) => return Err(e),
        }
    }
}

/// gzip-compress `data` (server /gzip endpoint).
pub fn gzip(data: &[u8]) -> io::Result<Vec<u8>> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(data)?;
    enc.finish()
}

/// gunzip `data` (mock body normalization, rule 7).
pub fn gunzip(data: &[u8]) -> io::Result<Vec<u8>> {
    use flate2::read::GzDecoder;
    let mut out = Vec::new();
    GzDecoder::new(data).read_to_end(&mut out)?;
    Ok(out)
}
