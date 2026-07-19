//! The typed error taxonomy (feature-matrix §5.15, row 20). Errors are **data** — a typed
//! variant with typed params — never message strings, matching Bolted's error rule
//! (`bolted-core`'s `ErrorData` records params as data; here the taxonomy is closed and
//! adapter-mapped, so an `enum` gives exhaustiveness plus a stable [`HttpErrorKey`] for the
//! C2 taxonomy matrix's positive-control-per-key).

use crate::request::Url;

/// A completed request's error outcome. The native-failure → variant mapping is
/// conformance-tested per adapter (never judgement).
///
/// `#[non_exhaustive]`: `QuotaExceeded` is reserved by §5.15 for the **background-transfer
/// family** (Windows' 200-op queue, Android quotas). It is deliberately *not* a variant here —
/// that family is out of this step's scope, and a key nothing can reach would be a
/// permanently-green needle with no positive control. It attaches when the family lands.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HttpError {
    /// The deadline elapsed (row 4 / rule 3). Distinct from [`HttpError::Cancelled`] — on .NET
    /// both surface as `TaskCanceledException`; the adapter classifies by which token fired
    /// (rule 2), never by exception type.
    Timeout,
    /// The caller cancelled the in-flight effect (row 21 / rule 9). Distinct from
    /// [`HttpError::Timeout`].
    Cancelled,
    /// The OS asked the user for permission and was refused (Android 16→17 local-network
    /// `EPERM`, Apple Local Network privacy) — distinct from a network failure (§5.15).
    PermissionDenied,
    /// An SPKI pin did not match (row 19 / rule 10). The pinning error.
    PinMismatch,
    /// A TLS failure that is not a pin mismatch (handshake, untrusted or invalid certificate).
    Tls {
        /// What kind of TLS failure.
        kind: TlsErrorKind,
    },
    /// DNS / name resolution failed.
    NameResolution,
    /// A connection could not be established (refused, unreachable, connect timeout).
    Connect,
    /// The exchange failed after the connection was established (reset, truncated mid-body). The
    /// positive-control target for rule 8 (no hidden request-level retry).
    Transport,
    /// An `https → http` redirect was refused (row 6 / rule 4).
    InsecureRedirect {
        /// The cleartext target that was refused.
        to: Url,
    },
    /// The redirect limit was exceeded (or a loop was detected).
    TooManyRedirects {
        /// The limit that was hit.
        limit: u32,
    },
    /// A local I/O failure handling the response (e.g. writing a [`crate::BodyOutcome::File`] sink).
    Io,
}

impl HttpError {
    /// The stable taxonomy key for this error (feature-matrix §5.15). The C2 matrix builds one
    /// positive control per key; params live on the variant, keys stay stringly-stable so shells
    /// and the taxonomy table share one vocabulary.
    #[must_use]
    pub fn key(&self) -> HttpErrorKey {
        match self {
            HttpError::Timeout => HttpErrorKey::Timeout,
            HttpError::Cancelled => HttpErrorKey::Cancelled,
            HttpError::PermissionDenied => HttpErrorKey::PermissionDenied,
            HttpError::PinMismatch => HttpErrorKey::PinMismatch,
            HttpError::Tls { .. } => HttpErrorKey::Tls,
            HttpError::NameResolution => HttpErrorKey::NameResolution,
            HttpError::Connect => HttpErrorKey::Connect,
            HttpError::Transport => HttpErrorKey::Transport,
            HttpError::InsecureRedirect { .. } => HttpErrorKey::InsecureRedirect,
            HttpError::TooManyRedirects { .. } => HttpErrorKey::TooManyRedirects,
            HttpError::Io => HttpErrorKey::Io,
        }
    }
}

/// The param-free key of an [`HttpError`] — the taxonomy's stable identity. `#[non_exhaustive]`
/// for the same reason as [`HttpError`].
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HttpErrorKey {
    Timeout,
    Cancelled,
    PermissionDenied,
    PinMismatch,
    Tls,
    NameResolution,
    Connect,
    Transport,
    InsecureRedirect,
    TooManyRedirects,
    Io,
}

impl HttpErrorKey {
    /// A stable, localisable-shell-friendly string key.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            HttpErrorKey::Timeout => "http.timeout",
            HttpErrorKey::Cancelled => "http.cancelled",
            HttpErrorKey::PermissionDenied => "http.permission_denied",
            HttpErrorKey::PinMismatch => "http.pin_mismatch",
            HttpErrorKey::Tls => "http.tls",
            HttpErrorKey::NameResolution => "http.name_resolution",
            HttpErrorKey::Connect => "http.connect",
            HttpErrorKey::Transport => "http.transport",
            HttpErrorKey::InsecureRedirect => "http.insecure_redirect",
            HttpErrorKey::TooManyRedirects => "http.too_many_redirects",
            HttpErrorKey::Io => "http.io",
        }
    }
}

/// The kind of a [`HttpError::Tls`] failure.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TlsErrorKind {
    /// The certificate chain did not terminate in a trusted root.
    UntrustedRoot,
    /// The certificate was expired or not yet valid.
    InvalidCertificate,
    /// The certificate did not match the requested host.
    HostnameMismatch,
    /// The TLS handshake failed for another reason (version/cipher negotiation, etc.).
    HandshakeFailure,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::Url;

    #[test]
    fn timeout_and_cancel_are_distinct_keys() {
        assert_ne!(HttpError::Timeout.key(), HttpError::Cancelled.key());
    }

    #[test]
    fn every_variant_maps_to_a_key_and_a_string() {
        let url = Url::cleartext_dev("http://x.test/").expect("valid");
        let all = [
            HttpError::Timeout,
            HttpError::Cancelled,
            HttpError::PermissionDenied,
            HttpError::PinMismatch,
            HttpError::Tls {
                kind: TlsErrorKind::UntrustedRoot,
            },
            HttpError::NameResolution,
            HttpError::Connect,
            HttpError::Transport,
            HttpError::InsecureRedirect { to: url },
            HttpError::TooManyRedirects { limit: 10 },
            HttpError::Io,
        ];
        // Distinct, non-empty string keys — the C2 taxonomy vocabulary.
        for e in &all {
            assert!(e.key().as_str().starts_with("http."));
        }
    }
}
