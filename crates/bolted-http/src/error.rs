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
    /// A streamed response body overflowed its bounded per-response ring (feature-matrix §5.11 /
    /// streaming-seam §3b): [`crate::stream::BodyStream::RING_CAPACITY`] undrained chunks were
    /// already buffered when another arrived. The typed failure that makes silent loss impossible —
    /// a conformant adapter pauses reading before it is hit (M2's capability signal); a broken one
    /// gets this instead of a dropped chunk.
    StreamOverflow {
        /// The ring capacity that was exceeded (core-owned; [`crate::stream::BodyStream::RING_CAPACITY`]).
        capacity: usize,
        /// The `seq` of the chunk that could not be buffered.
        seq: u64,
    },
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
            HttpError::StreamOverflow { .. } => HttpErrorKey::StreamOverflow,
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
    StreamOverflow,
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
            HttpErrorKey::StreamOverflow => "http.stream_overflow",
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

impl TlsErrorKind {
    /// A stable snake_case identifier — the value carried in the `kind` param of the
    /// [`HttpError::Tls`] → `ErrorData` bridge (a stable string a shell can localise, never Debug).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            TlsErrorKind::UntrustedRoot => "untrusted_root",
            TlsErrorKind::InvalidCertificate => "invalid_certificate",
            TlsErrorKind::HostnameMismatch => "hostname_mismatch",
            TlsErrorKind::HandshakeFailure => "handshake_failure",
        }
    }
}

/// The D1-shaped bridge from the HTTP taxonomy into `bolted-core`'s validation-error data (ruled
/// 2026-07-21 / Q6): **variant → snake_case key, fields → params**, exactly like every tier-1
/// `From<XError> for ErrorData` (fixture-note/-profile). The key is [`HttpErrorKey::as_str`] — the
/// `http.*` strings ARE the vocabulary, not a second one — and *every* data-carrying field becomes
/// a param, none dropped.
///
/// Behind the optional `bolted-core` feature so the default `bolted-http` build stays
/// dependency-free (the sans-io invariant); the composition root, which already links both crates,
/// enables it.
#[cfg(feature = "bolted-core")]
impl From<HttpError> for bolted_core::ErrorData {
    fn from(error: HttpError) -> Self {
        // The key is the taxonomy's own stable string — one vocabulary, shared with the shells.
        let key = error.key().as_str();
        let params: Vec<(&'static str, String)> = match error {
            HttpError::Tls { kind } => vec![("kind", kind.as_str().to_string())],
            HttpError::InsecureRedirect { to } => vec![("to", to.as_str().to_string())],
            HttpError::TooManyRedirects { limit } => vec![("limit", limit.to_string())],
            HttpError::StreamOverflow { capacity, seq } => {
                vec![("capacity", capacity.to_string()), ("seq", seq.to_string())]
            }
            // The param-free variants (Timeout, Cancelled, PermissionDenied, PinMismatch,
            // NameResolution, Connect, Transport, Io) carry a key only.
            _ => Vec::new(),
        };
        bolted_core::ErrorData { key, params }
    }
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
            HttpError::StreamOverflow {
                capacity: 256,
                seq: 256,
            },
        ];
        // Distinct, non-empty string keys — the C2 taxonomy vocabulary.
        for e in &all {
            assert!(e.key().as_str().starts_with("http."));
        }
    }

    #[test]
    fn stream_overflow_has_its_own_key() {
        let overflow = HttpError::StreamOverflow {
            capacity: 256,
            seq: 256,
        };
        assert_eq!(overflow.key(), HttpErrorKey::StreamOverflow);
        assert_eq!(overflow.key().as_str(), "http.stream_overflow");
        // Distinct from the transport failure it is not.
        assert_ne!(overflow.key(), HttpErrorKey::Transport);
    }

    // --- The HttpError -> ErrorData bridge (Q6), behind the `bolted-core` feature ------------

    #[cfg(feature = "bolted-core")]
    #[test]
    fn bridge_maps_a_param_carrying_variant_key_and_params() {
        // A multi-field variant: key from the taxonomy, every field a param, none dropped.
        let data: bolted_core::ErrorData = HttpError::StreamOverflow {
            capacity: 256,
            seq: 42,
        }
        .into();
        assert_eq!(data.key, "http.stream_overflow");
        assert_eq!(
            data.params,
            vec![("capacity", "256".to_string()), ("seq", "42".to_string()),]
        );

        // A single-field numeric variant.
        let data: bolted_core::ErrorData = HttpError::TooManyRedirects { limit: 7 }.into();
        assert_eq!(data.key, "http.too_many_redirects");
        assert_eq!(data.params, vec![("limit", "7".to_string())]);

        // A single-field enum variant carries its stable snake_case identifier, never Debug.
        let data: bolted_core::ErrorData = HttpError::Tls {
            kind: TlsErrorKind::HostnameMismatch,
        }
        .into();
        assert_eq!(data.key, "http.tls");
        assert_eq!(data.params, vec![("kind", "hostname_mismatch".to_string())]);

        // A single-field Url variant.
        let url = Url::cleartext_dev("http://legacy.test/x").expect("valid");
        let data: bolted_core::ErrorData = HttpError::InsecureRedirect { to: url }.into();
        assert_eq!(data.key, "http.insecure_redirect");
        assert_eq!(
            data.params,
            vec![("to", "http://legacy.test/x".to_string())]
        );
    }

    #[cfg(feature = "bolted-core")]
    #[test]
    fn bridge_maps_a_unit_variant_key_with_no_params() {
        let data: bolted_core::ErrorData = HttpError::Timeout.into();
        assert_eq!(data.key, "http.timeout");
        assert!(data.params.is_empty());

        let data: bolted_core::ErrorData = HttpError::Cancelled.into();
        assert_eq!(data.key, "http.cancelled");
        assert!(data.params.is_empty());
    }
}
