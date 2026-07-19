//! Mapping a `reqwest::Error` to the contract's typed [`HttpError`] taxonomy (feature-matrix §5.15
//! / rule 2). Classification is **by cause, not by exception shape**: a recorded TLS rejection wins
//! first (pin vs trust), then timeout, then the connect-phase split (refused vs DNS), then the
//! catch-all transport failure.
//!
//! reqwest exposes no stable typed DNS error, so name-resolution is recognised by walking the error
//! `source` chain (a `std::io::Error` kind for the refused case, message markers for DNS). This is
//! recorded friction — the honest seam reqwest gives us — not a shortcut.

use std::fmt::Write as _;
use std::io;

use bolted_http::HttpError;

use crate::tls::TlsReject;

/// Map a completed-request `reqwest::Error` to a typed [`HttpError`], given any TLS rejection the
/// pinning verifier recorded out-of-band during the (now-failed) handshake.
pub(crate) fn map_reqwest_error(err: &reqwest::Error, reject: Option<TlsReject>) -> HttpError {
    // A recorded TLS reason is the most specific signal (pin mismatch vs untrusted/hostname).
    if let Some(reject) = reject {
        return reject.into_error();
    }
    if err.is_timeout() {
        return HttpError::Timeout;
    }

    // Walk the source chain: capture a refused-connection io kind and the concatenated message.
    let mut msg = String::new();
    let mut refused = false;
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(err);
    while let Some(cur) = source {
        let _ = write!(msg, "{cur}; ");
        if let Some(io_err) = cur.downcast_ref::<io::Error>()
            && io_err.kind() == io::ErrorKind::ConnectionRefused
        {
            refused = true;
        }
        source = cur.source();
    }
    let low = msg.to_ascii_lowercase();

    if refused {
        return HttpError::Connect;
    }
    if low.contains("dns")
        || low.contains("failed to lookup")
        || low.contains("lookup address")
        || low.contains("name or service not known")
        || low.contains("nodename")
        || low.contains("no such host")
    {
        return HttpError::NameResolution;
    }
    if err.is_connect() {
        // A connect-phase TLS failure that never reached our verifier (protocol/handshake).
        if low.contains("certificate") || low.contains("tls") || low.contains("handshake") {
            return HttpError::Tls {
                kind: bolted_http::TlsErrorKind::HandshakeFailure,
            };
        }
        return HttpError::Connect;
    }
    // Anything after the connection was established (reset, truncated mid-body): the rule-8 target.
    HttpError::Transport
}
