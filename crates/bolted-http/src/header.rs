//! Typed header names and values, and the reserved-header guard (feature-matrix §5.1, rule 6).
//!
//! Two name types, on purpose:
//!
//! - [`HeaderName`] — any valid HTTP token. Used for **response** headers (the adapter reports
//!   whatever the wire carried).
//! - [`RequestHeaderName`] — a name permitted on a **request**. The adapter-owned reserved set
//!   (`Host`, `Content-Length`, `Connection`, …) is **not constructible** through this type's
//!   `const` constructor: a reserved literal is a compile error (rule 6's core half), and the
//!   runtime constructor returns an error rather than silently dropping the header.

/// The adapter-owned reserved header set (feature-matrix §5.1). Lowercase for case-insensitive
/// comparison. Core-set violations on a request are a **compile error**, never a runtime drop.
///
/// `Authorization` is deliberately *not* here — it is settable everywhere; the cross-origin-redirect
/// strip is an adapter rule (rule 4 neighbourhood), not a construction restriction.
const RESERVED: &[&str] = &[
    "host",
    "content-length",
    "connection",
    "transfer-encoding",
    "keep-alive",
    "proxy-connection",
    "upgrade",
    "accept-encoding",
    "cookie",
];

const fn to_ascii_lower(c: u8) -> u8 {
    if c.is_ascii_uppercase() { c + 32 } else { c }
}

const fn ascii_eq_ignore_case(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if to_ascii_lower(a[i]) != to_ascii_lower(b[i]) {
            return false;
        }
        i += 1;
    }
    true
}

/// Whether `name` is in the adapter-owned reserved set (case-insensitive).
const fn is_reserved(name: &str) -> bool {
    let name = name.as_bytes();
    let mut i = 0;
    while i < RESERVED.len() {
        if ascii_eq_ignore_case(name, RESERVED[i].as_bytes()) {
            return true;
        }
        i += 1;
    }
    false
}

/// Whether `name` is a non-empty RFC 7230 header token (visible ASCII, no separators).
const fn is_valid_token(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    let mut i = 0;
    while i < bytes.len() {
        if !is_token_byte(bytes[i]) {
            return false;
        }
        i += 1;
    }
    true
}

const fn is_token_byte(b: u8) -> bool {
    matches!(b,
        b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.'
        | b'^' | b'_' | b'`' | b'|' | b'~'
        | b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z')
}

/// A valid HTTP header name (any token). Case-insensitive equality.
#[derive(Clone, Debug)]
pub struct HeaderName(NameRepr);

#[derive(Clone, Debug)]
enum NameRepr {
    Static(&'static str),
    Owned(Box<str>),
}

impl HeaderName {
    /// Compile-time constructor for a literal name. An invalid token is a **compile error**
    /// (const-eval), so this can only ever produce a well-formed name.
    #[must_use]
    pub const fn from_static(name: &'static str) -> Self {
        assert!(is_valid_token(name), "not a valid HTTP header token");
        HeaderName(NameRepr::Static(name))
    }

    /// Runtime constructor for a dynamic name. Returns an error (never panics) for a bad token.
    pub fn parse(name: &str) -> Result<Self, InvalidHeaderName> {
        if is_valid_token(name) {
            Ok(HeaderName(NameRepr::Owned(name.into())))
        } else {
            Err(InvalidHeaderName)
        }
    }

    /// The name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match &self.0 {
            NameRepr::Static(s) => s,
            NameRepr::Owned(s) => s,
        }
    }
}

impl PartialEq for HeaderName {
    fn eq(&self, other: &Self) -> bool {
        self.as_str().eq_ignore_ascii_case(other.as_str())
    }
}
impl Eq for HeaderName {}

/// A header name is not a valid HTTP token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidHeaderName;

/// A header name permitted on a **request**. The reserved set (feature-matrix §5.1) is
/// adapter-owned and cannot be constructed here.
///
/// The core-set guard is a **compile error**, demonstrated by these doctests:
///
/// ```
/// // A custom name compiles:
/// const OK: bolted_http::RequestHeaderName =
///     bolted_http::RequestHeaderName::from_static("X-Trace-Id");
/// ```
///
/// ```compile_fail
/// // A reserved name is rejected at compile time (rule 6's core half):
/// const BAD: bolted_http::RequestHeaderName =
///     bolted_http::RequestHeaderName::from_static("Host");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestHeaderName(HeaderName);

impl RequestHeaderName {
    /// Compile-time constructor. A reserved or invalid literal is a **compile error**; this is
    /// the only infallible path, so reserved names are unconstructible on requests.
    #[must_use]
    pub const fn from_static(name: &'static str) -> Self {
        assert!(
            !is_reserved(name),
            "reserved (adapter-owned) header name is not permitted on a request"
        );
        // Delegates token validation (also a compile error on a bad token).
        RequestHeaderName(HeaderName::from_static(name))
    }

    /// Runtime constructor. A reserved or invalid name returns an error — never a panic, never a
    /// silent drop.
    pub fn parse(name: &str) -> Result<Self, RequestHeaderError> {
        if is_reserved(name) {
            return Err(RequestHeaderError::Reserved);
        }
        HeaderName::parse(name)
            .map(RequestHeaderName)
            .map_err(|_| RequestHeaderError::InvalidToken)
    }

    /// Borrow as the general [`HeaderName`].
    #[must_use]
    pub fn as_header(&self) -> &HeaderName {
        &self.0
    }

    /// The name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Why a dynamic request header name was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestHeaderError {
    /// The name is in the adapter-owned reserved set (feature-matrix §5.1).
    Reserved,
    /// The name is not a valid HTTP token.
    InvalidToken,
}

/// A header value: opaque bytes (no interior CR/LF).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderValue(Box<[u8]>);

impl HeaderValue {
    /// Construct from bytes, rejecting embedded CR/LF (header injection).
    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Result<Self, InvalidHeaderValue> {
        let bytes = bytes.into();
        if bytes.iter().any(|&b| b == b'\r' || b == b'\n') {
            return Err(InvalidHeaderValue);
        }
        Ok(HeaderValue(bytes.into_boxed_slice()))
    }

    /// Construct from a string slice (convenience over [`HeaderValue::from_bytes`]).
    pub fn from_text(value: &str) -> Result<Self, InvalidHeaderValue> {
        Self::from_bytes(value.as_bytes().to_vec())
    }

    /// The raw value bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// A header value contained a forbidden byte (CR or LF).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidHeaderValue;

/// An ordered, multi-value-capable set of **request** headers. Reserved names are excluded by
/// the [`RequestHeaderName`] type, so this container needs no runtime reserved check.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RequestHeaders(Vec<(RequestHeaderName, HeaderValue)>);

impl RequestHeaders {
    /// An empty header set.
    #[must_use]
    pub fn new() -> Self {
        RequestHeaders(Vec::new())
    }

    /// Append a header (duplicates are preserved, in order).
    pub fn append(&mut self, name: RequestHeaderName, value: HeaderValue) {
        self.0.push((name, value));
    }

    /// Iterate the headers in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&RequestHeaderName, &HeaderValue)> {
        self.0.iter().map(|(n, v)| (n, v))
    }

    /// The number of header entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there are no headers.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// An ordered, multi-value-capable set of **response** headers (any name).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Headers(Vec<(HeaderName, HeaderValue)>);

impl Headers {
    /// An empty header set.
    #[must_use]
    pub fn new() -> Self {
        Headers(Vec::new())
    }

    /// Append a header (duplicates are preserved, in order).
    pub fn append(&mut self, name: HeaderName, value: HeaderValue) {
        self.0.push((name, value));
    }

    /// The first value whose name matches `name` case-insensitively.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&HeaderValue> {
        self.0
            .iter()
            .find(|(n, _)| n.as_str().eq_ignore_ascii_case(name))
            .map(|(_, v)| v)
    }

    /// Iterate the headers in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&HeaderName, &HeaderValue)> {
        self.0.iter().map(|(n, v)| (n, v))
    }

    /// The number of header entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there are no headers.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_request_header_parses() {
        assert!(RequestHeaderName::parse("X-Trace-Id").is_ok());
    }

    #[test]
    fn reserved_request_header_is_rejected_at_runtime() {
        // The compile-time half is proven by the `compile_fail` doctest on `RequestHeaderName`.
        assert_eq!(
            RequestHeaderName::parse("Host"),
            Err(RequestHeaderError::Reserved)
        );
        assert_eq!(
            RequestHeaderName::parse("content-length"),
            Err(RequestHeaderError::Reserved)
        );
    }

    #[test]
    fn header_names_compare_case_insensitively() {
        assert_eq!(
            HeaderName::from_static("ETag"),
            HeaderName::from_static("etag")
        );
    }

    #[test]
    fn header_value_rejects_crlf() {
        assert_eq!(HeaderValue::from_text("a\r\nb"), Err(InvalidHeaderValue));
        assert!(HeaderValue::from_text("application/json").is_ok());
    }
}
