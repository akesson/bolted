//! The declarative SPKI pinning verifier (feature-matrix row 19 / rule 10 — the step-24 M3 **L2**
//! verdict).
//!
//! **The verdict is: pinning works on Linux/reqwest, cleanly, via `use_preconfigured_tls`.**
//! [`PinningVerifier`] is a `rustls::ClientConfig` custom [`ServerCertVerifier`] that:
//!
//! 1. delegates the **trust decision** to a real [`WebPkiServerVerifier`] — genuine chain building
//!    and hostname matching against the configured trust anchors (no allowlist shortcut, unlike the
//!    conformance socket mock); then
//! 2. **additionally** enforces the request's SHA-256-of-SPKI pins on the end-entity certificate.
//!
//! A pin mismatch is [`HttpError::PinMismatch`]; a trust/hostname failure is [`HttpError::Tls`].
//! rustls collapses every rejection into one opaque handshake error, so — exactly as the socket
//! mock does — the verifier records *why* it rejected into a shared slot the adapter reads after the
//! handshake fails (the reqwest error alone cannot carry the distinction).
//!
//! The rustls verifier API expresses this without obstruction: `WebPkiServerVerifier::builder`
//! yields an `Arc<dyn ServerCertVerifier>` we wrap, and `ClientConfig::dangerous()
//! .with_custom_certificate_verifier` installs the wrapper. Row 19 stands for Linux (no demote).

use std::sync::{Arc, Mutex};

use bolted_http::{HttpError, PinSet, TlsErrorKind};
use rustls::CertificateError;
use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, RootCertStore, SignatureScheme};
use sha2::{Digest, Sha256};

/// Why the verifier rejected the server certificate — recorded out-of-band so the adapter can map
/// the opaque rustls/reqwest handshake failure back to a typed [`HttpError`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TlsReject {
    /// A declarative SPKI pin did not match (rule 10) — chain/hostname verification *passed*.
    Pin,
    /// Real chain/hostname verification failed (untrusted root, bad hostname, expired, …).
    Cert(TlsErrorKind),
}

impl TlsReject {
    /// The typed error this rejection maps to.
    pub(crate) fn into_error(self) -> HttpError {
        match self {
            TlsReject::Pin => HttpError::PinMismatch,
            TlsReject::Cert(kind) => HttpError::Tls { kind },
        }
    }
}

/// A shared, single-request slot the verifier writes its rejection reason into.
pub(crate) type RejectSlot = Arc<Mutex<Option<TlsReject>>>;

/// SHA-256 of the certificate's SubjectPublicKeyInfo — the honest SPKI pin (the same computation the
/// harness server and socket mock use). A parse failure surfaces as `None`.
fn spki_sha256(cert_der: &[u8]) -> Option<[u8; 32]> {
    let (_, cert) = x509_parser::parse_x509_certificate(cert_der).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(cert.public_key().raw);
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    Some(out)
}

/// Map a rustls verification error to the typed TLS reason we record.
fn classify(err: &rustls::Error) -> TlsErrorKind {
    match err {
        rustls::Error::InvalidCertificate(ce) => match ce {
            CertificateError::UnknownIssuer => TlsErrorKind::UntrustedRoot,
            CertificateError::Expired | CertificateError::ExpiredContext { .. } => {
                TlsErrorKind::InvalidCertificate
            }
            CertificateError::NotValidYet | CertificateError::NotValidYetContext { .. } => {
                TlsErrorKind::InvalidCertificate
            }
            CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. } => {
                TlsErrorKind::HostnameMismatch
            }
            _ => TlsErrorKind::HandshakeFailure,
        },
        _ => TlsErrorKind::HandshakeFailure,
    }
}

/// The custom rustls verifier: real trust decision (delegated) + declarative SPKI pinning on top.
#[derive(Debug)]
pub(crate) struct PinningVerifier {
    inner: Arc<WebPkiServerVerifier>,
    pins: Option<Vec<[u8; 32]>>,
    enforce_pins: bool,
    reject: RejectSlot,
}

impl PinningVerifier {
    /// Build a verifier over `roots` (real trust anchors) carrying the request's `pins`. When
    /// `enforce_pins` is `false`, pinning is skipped (the scoped red-twin: a pin no longer bites).
    /// Fails only if `roots` cannot yield a webpki verifier (e.g. no anchors).
    pub(crate) fn new(
        roots: Arc<RootCertStore>,
        provider: Arc<CryptoProvider>,
        pins: Option<&PinSet>,
        enforce_pins: bool,
        reject: RejectSlot,
    ) -> Result<Self, HttpError> {
        let inner = WebPkiServerVerifier::builder_with_provider(roots, provider)
            .build()
            .map_err(|_| HttpError::Tls {
                kind: TlsErrorKind::HandshakeFailure,
            })?;
        let pins = pins.map(|set| set.pins().iter().map(|p| *p.as_bytes()).collect());
        Ok(PinningVerifier {
            inner,
            pins,
            enforce_pins,
            reject,
        })
    }

    fn record(&self, why: TlsReject) -> rustls::Error {
        if let Ok(mut slot) = self.reject.lock() {
            *slot = Some(why);
        }
        rustls::Error::InvalidCertificate(CertificateError::ApplicationVerificationFailure)
    }
}

impl ServerCertVerifier for PinningVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        // 1. The REAL trust decision: chain building + hostname matching against the anchors.
        if let Err(err) = self.inner.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        ) {
            return Err(self.record(TlsReject::Cert(classify(&err))));
        }

        // 2. The declarative SPKI pins, ANDed on top of a passing chain (rule 10).
        if self.enforce_pins
            && let Some(pins) = &self.pins
        {
            let spki = spki_sha256(end_entity.as_ref())
                .ok_or_else(|| rustls::Error::InvalidCertificate(CertificateError::BadEncoding))?;
            if !pins.contains(&spki) {
                return Err(self.record(TlsReject::Pin));
            }
        }
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}
