//! The response: [`HttpResponse`] and its parts (status, headers, body-sink outcome, final URL,
//! redirect hop trace, negotiated version).

use crate::header::Headers;
use crate::request::{FileRef, Url};

/// An HTTP status code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusCode(u16);

impl StatusCode {
    /// `200 OK`.
    pub const OK: StatusCode = StatusCode(200);

    /// Wrap a status code.
    #[must_use]
    pub const fn new(code: u16) -> Self {
        StatusCode(code)
    }

    /// The numeric code.
    #[must_use]
    pub const fn as_u16(self) -> u16 {
        self.0
    }

    /// Whether the code is 2xx.
    #[must_use]
    pub const fn is_success(self) -> bool {
        self.0 >= 200 && self.0 < 300
    }
}

/// The negotiated HTTP version (feature-matrix row 11). A plain observable — **not** an
/// `Option`: all four native surfaces always report it (the `Option` in the first draft was
/// web's, §5.7). `#[non_exhaustive]` for future versions.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpVersion {
    Http1_0,
    Http1_1,
    Http2,
    Http3,
}

/// Where the response body landed (feature-matrix row 15, the sink model). `#[non_exhaustive]`:
/// this is the **extension seam for response streaming** (row 16, `CORE, gated` on the S-FFI
/// verdict). If chunked delivery survives the FFI probe, a streaming variant attaches here
/// without touching the rest of the contract; the sink model stands regardless.
#[non_exhaustive]
#[derive(Clone, Debug)]
pub enum BodyOutcome {
    /// The decoded body, buffered in memory.
    Memory(Vec<u8>),
    /// The body was sunk to a file without buffering (Apple `downloadTask`; synthesised by copy
    /// on the other three surfaces, §5.10).
    File(FileRef),
    // Streaming delivery (row 16) attaches here once S-FFI picks the FFI mechanism (§5.11).
}

/// A completed HTTP response (feature-matrix §5). Redirects are already followed; bodies are
/// already decoded (§5.12).
#[derive(Clone, Debug)]
pub struct HttpResponse {
    status: StatusCode,
    headers: Headers,
    body: BodyOutcome,
    final_url: Url,
    hops: Vec<Url>,
    version: HttpVersion,
    content_length: Option<u64>,
}

impl HttpResponse {
    /// Start building a response. Status, final URL, version and body outcome are required (the
    /// version is non-optional by row 11).
    pub fn builder(
        status: StatusCode,
        final_url: Url,
        version: HttpVersion,
        body: BodyOutcome,
    ) -> ResponseBuilder {
        ResponseBuilder {
            status,
            headers: Headers::new(),
            body,
            final_url,
            hops: Vec::new(),
            version,
            content_length: None,
        }
    }

    /// The status code.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// The response headers.
    #[must_use]
    pub fn headers(&self) -> &Headers {
        &self.headers
    }

    /// The body-sink outcome (row 15).
    #[must_use]
    pub fn body(&self) -> &BodyOutcome {
        &self.body
    }

    /// The final URL after any redirects (row 6).
    #[must_use]
    pub fn final_url(&self) -> &Url {
        &self.final_url
    }

    /// The redirect hop trace (row 7) — the URLs traversed, in order, excluding the final URL.
    /// Empty when no redirect occurred.
    #[must_use]
    pub fn hops(&self) -> &[Url] {
        &self.hops
    }

    /// The negotiated HTTP version (row 11, always present).
    #[must_use]
    pub fn version(&self) -> HttpVersion {
        self.version
    }

    /// The advisory body length (feature-matrix row 13/17, §5.12). **`None`-or-honest**: bodies are
    /// always decoded, and a transport length that would lie under decoding (gzip/brotli/zstd strip
    /// or invalidate `Content-Length`) is reported as `None`, never the compressed figure. `Some(n)`
    /// is a promise the delivered body is `n` decoded bytes. Never a "raw body" length.
    #[must_use]
    pub fn content_length(&self) -> Option<u64> {
        self.content_length
    }
}

/// Builder for [`HttpResponse`].
#[derive(Clone, Debug)]
#[must_use = "call `.build()` to produce the response"]
pub struct ResponseBuilder {
    status: StatusCode,
    headers: Headers,
    body: BodyOutcome,
    final_url: Url,
    hops: Vec<Url>,
    version: HttpVersion,
    content_length: Option<u64>,
}

impl ResponseBuilder {
    /// Set the headers.
    pub fn headers(mut self, headers: Headers) -> Self {
        self.headers = headers;
        self
    }

    /// Record a redirect hop (in traversal order).
    pub fn hop(mut self, url: Url) -> Self {
        self.hops.push(url);
        self
    }

    /// Set the advisory decoded body length (row 13/17). `None` (the default) is the honest answer
    /// whenever a transport length would be a lie under decoding (§5.12).
    pub fn content_length(mut self, content_length: Option<u64>) -> Self {
        self.content_length = content_length;
        self
    }

    /// Finish building.
    #[must_use]
    pub fn build(self) -> HttpResponse {
        HttpResponse {
            status: self.status,
            headers: self.headers,
            body: self.body,
            final_url: self.final_url,
            hops: self.hops,
            version: self.version,
            content_length: self.content_length,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_success_predicate() {
        assert!(StatusCode::OK.is_success());
        assert!(!StatusCode::new(500).is_success());
        assert_eq!(StatusCode::OK.as_u16(), 200);
    }

    #[test]
    fn response_builder_defaults_are_empty() {
        let url = Url::https("https://example.test/").expect("valid url");
        let resp = HttpResponse::builder(
            StatusCode::OK,
            url,
            HttpVersion::Http2,
            BodyOutcome::Memory(Vec::new()),
        )
        .build();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.version(), HttpVersion::Http2);
        assert!(resp.hops().is_empty());
        assert!(resp.headers().is_empty());
        assert_eq!(resp.content_length(), None);
    }

    #[test]
    fn content_length_is_none_or_honest() {
        let url = Url::https("https://example.test/").expect("valid url");
        let honest = HttpResponse::builder(
            StatusCode::OK,
            url,
            HttpVersion::Http2,
            BodyOutcome::Memory(vec![0u8; 12]),
        )
        .content_length(Some(12))
        .build();
        assert_eq!(honest.content_length(), Some(12));
    }
}
