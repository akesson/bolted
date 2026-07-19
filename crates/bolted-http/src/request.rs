//! The request effect: [`HttpRequest`] plus its parts (method, URL, body, one total deadline)
//! and the declarative capability data carried on it (priority hint, SPKI pins).
//!
//! An `HttpRequest` is **complete data handed over once** — there is no streaming request body
//! (feature-matrix §5.3, OUT by design); large uploads are [`RequestBody::File`].

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::header::{HeaderValue, RequestHeaderName, RequestHeaders};

/// The HTTP method (feature-matrix row 1). `#[non_exhaustive]`: uncommon methods can be added
/// without a breaking change.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
    Options,
}

/// A request URL. HTTPS-only by default (feature-matrix row 10 / §5.15): the sans-io core can
/// refuse a non-`https` URL *before* the effect is emitted, which makes the rule uniform without
/// any adapter. Cleartext is a dev-gated exception via [`Url::cleartext_dev`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Url(String);

impl Url {
    /// Construct an HTTPS URL. Any other scheme is rejected (row 10).
    pub fn https(url: &str) -> Result<Self, UrlError> {
        if url.is_empty() {
            return Err(UrlError::Empty);
        }
        if starts_with_ignore_case(url, "https://") {
            Ok(Url(url.to_owned()))
        } else {
            Err(UrlError::NotHttps)
        }
    }

    /// The dev-only cleartext (`http://`) exception (row 10). Named so misuse is visible at the
    /// call site; a shipped adapter gates it at the composition root.
    pub fn cleartext_dev(url: &str) -> Result<Self, UrlError> {
        if url.is_empty() {
            return Err(UrlError::Empty);
        }
        if starts_with_ignore_case(url, "http://") {
            Ok(Url(url.to_owned()))
        } else {
            Err(UrlError::NotCleartext)
        }
    }

    /// The URL as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn starts_with_ignore_case(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len() && s.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

/// Why a URL was rejected (typed — never a message string).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UrlError {
    /// The URL was empty.
    Empty,
    /// [`Url::https`] was given a non-`https` URL.
    NotHttps,
    /// [`Url::cleartext_dev`] was given a non-`http` URL.
    NotCleartext,
}

/// An opaque reference to a file, the large-body / file-sink primitive (feature-matrix §5.10).
///
/// A newtype over a path today (a file is a path on all four native surfaces). Kept
/// **opaque-ready** — the inner path is private — because a future web adapter would reinterpret
/// it as an OPFS handle, and the background-transfer family wants the same type (§5.10, §9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRef(PathBuf);

impl FileRef {
    /// Wrap a path.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        FileRef(path.into())
    }

    /// Borrow the path. (A web adapter would not expose this; hence "opaque-ready".)
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

/// A request body (feature-matrix row 2). `#[non_exhaustive]`: `Multipart{parts: Bytes|File}` is
/// the next variant (row 2, CORE(adapter)); it attaches here without a breaking change.
#[non_exhaustive]
#[derive(Clone, Debug)]
pub enum RequestBody {
    /// No body.
    Empty,
    /// An in-memory body.
    Bytes(Vec<u8>),
    /// A file body, streamed from disk by the adapter (§5.2).
    File(FileRef),
}

/// A priority hint (feature-matrix row 12). **CAP** (decided 2026-07-19): honored only where an
/// adapter implements [`crate::PriorityHint`] (Apple, Cronet/HttpEngine); legally ignored
/// elsewhere. The hint *data* rides every request regardless; the trait signals honouring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Priority {
    Throttled,
    Low,
    Normal,
    High,
    Critical,
}

/// A SHA-256 SPKI pin: the hash of a certificate's `SubjectPublicKeyInfo` (DER). Declarative
/// pin **data** (feature-matrix §5.14, row 19 — CORE(adapter), Linux feasibility spike-gated at
/// L2). No callbacks: the contract carries values, the adapter maps them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpkiPin([u8; 32]);

impl SpkiPin {
    /// Construct from a raw SHA-256 digest of the SubjectPublicKeyInfo.
    #[must_use]
    pub const fn from_sha256(digest: [u8; 32]) -> Self {
        SpkiPin(digest)
    }

    /// The 32-byte digest.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// A set of acceptable SPKI pins for a request (any one matching satisfies the pin — the standard
/// backup-pin shape).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PinSet(Vec<SpkiPin>);

impl PinSet {
    /// A pin set from acceptable pins.
    #[must_use]
    pub fn new(pins: Vec<SpkiPin>) -> Self {
        PinSet(pins)
    }

    /// The acceptable pins.
    #[must_use]
    pub fn pins(&self) -> &[SpkiPin] {
        &self.0
    }

    /// Whether the set is empty (no pinning requested).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A complete, cancellable HTTP request effect (feature-matrix §2, §5). Cookie-less and
/// cache-less by construction — it carries no ambient state.
#[derive(Clone, Debug)]
pub struct HttpRequest {
    method: Method,
    url: Url,
    headers: RequestHeaders,
    body: RequestBody,
    deadline: Duration,
    priority: Option<Priority>,
    pins: Option<PinSet>,
}

impl HttpRequest {
    /// Start building a request. The **total deadline** is required up front (row 4): it comes
    /// from the caller/core, never from an adapter literal — the one timeout every surface honors.
    pub fn builder(method: Method, url: Url, deadline: Duration) -> RequestBuilder {
        RequestBuilder {
            method,
            url,
            headers: RequestHeaders::new(),
            body: RequestBody::Empty,
            deadline,
            priority: None,
            pins: None,
        }
    }

    /// The method.
    #[must_use]
    pub fn method(&self) -> Method {
        self.method
    }

    /// The URL.
    #[must_use]
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// The request headers.
    #[must_use]
    pub fn headers(&self) -> &RequestHeaders {
        &self.headers
    }

    /// The body.
    #[must_use]
    pub fn body(&self) -> &RequestBody {
        &self.body
    }

    /// The total deadline (a budget; the adapter arms the timer and cancels on expiry).
    #[must_use]
    pub fn deadline(&self) -> Duration {
        self.deadline
    }

    /// The priority hint, if any (CAP — row 12).
    #[must_use]
    pub fn priority(&self) -> Option<Priority> {
        self.priority
    }

    /// The SPKI pins, if any (row 19).
    #[must_use]
    pub fn pins(&self) -> Option<&PinSet> {
        self.pins.as_ref()
    }
}

/// Builder for [`HttpRequest`].
#[derive(Clone, Debug)]
#[must_use = "call `.build()` to produce the request"]
pub struct RequestBuilder {
    method: Method,
    url: Url,
    headers: RequestHeaders,
    body: RequestBody,
    deadline: Duration,
    priority: Option<Priority>,
    pins: Option<PinSet>,
}

impl RequestBuilder {
    /// Append a header. Reserved names are unrepresentable (the [`RequestHeaderName`] type).
    pub fn header(mut self, name: RequestHeaderName, value: HeaderValue) -> Self {
        self.headers.append(name, value);
        self
    }

    /// Set the body.
    pub fn body(mut self, body: RequestBody) -> Self {
        self.body = body;
        self
    }

    /// Set the priority hint (row 12, CAP).
    pub fn priority(mut self, priority: Priority) -> Self {
        self.priority = Some(priority);
        self
    }

    /// Set the SPKI pins (row 19).
    pub fn pins(mut self, pins: PinSet) -> Self {
        self.pins = Some(pins);
        self
    }

    /// Finish building.
    #[must_use]
    pub fn build(self) -> HttpRequest {
        HttpRequest {
            method: self.method,
            url: self.url,
            headers: self.headers,
            body: self.body,
            deadline: self.deadline,
            priority: self.priority,
            pins: self.pins,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_required_by_default() {
        assert!(Url::https("https://example.test/x").is_ok());
        assert_eq!(Url::https("http://example.test/x"), Err(UrlError::NotHttps));
        assert_eq!(Url::https(""), Err(UrlError::Empty));
    }

    #[test]
    fn cleartext_is_a_named_exception() {
        assert!(Url::cleartext_dev("http://localhost:8080/x").is_ok());
        assert_eq!(
            Url::cleartext_dev("https://example.test/x"),
            Err(UrlError::NotCleartext)
        );
    }

    #[test]
    fn builder_carries_deadline_and_caps() {
        let url = Url::https("https://example.test/").expect("valid url");
        let req = HttpRequest::builder(Method::Get, url, Duration::from_secs(30))
            .priority(Priority::High)
            .pins(PinSet::new(vec![SpkiPin::from_sha256([0u8; 32])]))
            .build();
        assert_eq!(req.method(), Method::Get);
        assert_eq!(req.deadline(), Duration::from_secs(30));
        assert_eq!(req.priority(), Some(Priority::High));
        assert_eq!(req.pins().map(PinSet::pins).map(<[_]>::len), Some(1));
    }
}
