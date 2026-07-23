//! `bolted-http` — the platform-neutral HTTP capability contract.
//!
//! Sans-io: this crate defines the [`Http`] capability trait and the typed request /
//! response / error data that cross the effect boundary. Execution always belongs to a
//! platform adapter over the native stack (URLSession, OkHttp/Cronet, WinHTTP/WinRT
//! BackgroundTransfer, libcurl/reqwest on Linux) — never to this crate. The lib target has
//! **no** tokio/reqwest/TLS dependency; it is data + traits.
//!
//! The surface is derived from `docs/feature-matrix.md` (§4 classification, §5 per-row
//! evidence, §7 the eleven conformance rules); where the older `architecture.md` §2 sketch
//! and the matrix differ, the matrix wins.
//!
//! ## The `Send` seam (target-conditional bounds)
//!
//! Every trait bound that would be `Send` on a native target is written against the
//! [`MaybeSend`] alias instead. On non-wasm targets `MaybeSend` *is* `Send`; on `wasm32` it
//! is empty (wasm futures are `!Send`). This is the **single point of change** a future web
//! adapter would need — the trait signatures never mention `Send` directly, so no signature
//! changes when wasm relaxes them. No wasm target is built in this step; the seam only has to
//! be the one place the relaxation lives. (feature-matrix §9.1, decided 2026-07-19.)
#![forbid(unsafe_code)]

pub mod capability;
pub mod error;
pub mod header;
pub mod redirect;
pub mod request;
pub mod response;
pub mod signal;
pub mod stream;

#[cfg(feature = "conformance")]
pub mod conformance;

pub use capability::{
    CancelToken, ChunkSink, CompletionSink, Http, Metrics, MetricsTier, RequestHandle,
    StreamingHttp, UploadProgressSink,
};
pub use error::{HttpError, HttpErrorKey, TlsErrorKind};
pub use header::{
    HeaderName, HeaderValue, Headers, InvalidHeaderName, InvalidHeaderValue, RequestHeaderError,
    RequestHeaderName, RequestHeaders,
};
pub use redirect::RedirectCeiling;
pub use request::{
    FileRef, HttpRequest, Method, PinSet, Priority, RequestBody, RequestBuilder, ResponseSink,
    SpkiPin, Url, UrlError,
};
pub use response::{BodyOutcome, HttpResponse, HttpVersion, ResponseBuilder, StatusCode};
pub use signal::{FlowObserver, FlowSignal, FlowSignals};
pub use stream::{BodyChunk, BodyEnd, BodyStream};

// --- The Send seam ---------------------------------------------------------------------
//
// The only target-conditional code in the crate. Traits that must be `Send`-able on native
// targets bound their implementors on `MaybeSend`; this module decides what that costs.

#[cfg(not(target_arch = "wasm32"))]
mod send_seam {
    /// Native alias for `Send`. See the crate-level "The `Send` seam" note.
    pub trait MaybeSend: Send {}
    impl<T: Send + ?Sized> MaybeSend for T {}
}

#[cfg(target_arch = "wasm32")]
mod send_seam {
    /// wasm alias: empty (wasm futures are `!Send`). See the crate-level "The `Send` seam" note.
    pub trait MaybeSend {}
    impl<T: ?Sized> MaybeSend for T {}
}

pub use send_seam::MaybeSend;
