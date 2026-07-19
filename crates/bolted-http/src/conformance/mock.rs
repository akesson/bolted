//! The in-memory mock adapter (feature `conformance`). Implements [`Http`] with no I/O: `send`
//! computes the outcome from a scripted closure and delivers it synchronously. It is the vehicle
//! for watching every row fail correctly before a real adapter is trusted to pass.

use std::sync::Arc;

use super::AdapterFactory;
use crate::capability::{CancelToken, CompletionSink, Http, RequestHandle, UploadProgressSink};
use crate::error::HttpError;
use crate::request::HttpRequest;
use crate::response::{BodyOutcome, HttpResponse, HttpVersion, StatusCode};

/// The script an adapter runs per request. `None` means "do not complete" (a silent adapter, for
/// testing the harness's no-completion path).
type Script = Arc<dyn Fn(&HttpRequest) -> Option<Result<HttpResponse, HttpError>> + Send + Sync>;

/// A scriptable, in-memory [`Http`] adapter. No sockets, no runtime: `send` runs the script and
/// delivers the outcome on the calling thread.
#[derive(Clone)]
pub struct MockHttp {
    script: Script,
}

impl MockHttp {
    /// A mock whose per-request outcome is `script(request)`; `None` completes nothing.
    pub fn new(
        script: impl Fn(&HttpRequest) -> Option<Result<HttpResponse, HttpError>> + Send + Sync + 'static,
    ) -> Self {
        MockHttp {
            script: Arc::new(script),
        }
    }
}

impl Http for MockHttp {
    fn send(
        &self,
        request: HttpRequest,
        completion: Box<dyn CompletionSink>,
        _upload_progress: Option<Box<dyn UploadProgressSink>>,
    ) -> RequestHandle {
        // The scripted mock performs no upload; it ignores the progress sink (a legal choice for an
        // adapter with no body to hand off). The socket mock exercises the progress surface.
        let token = CancelToken::new();
        if let Some(outcome) = (self.script)(&request) {
            completion.complete(outcome);
        }
        // Silent scripts drop `completion` without delivering — exercises the harness's
        // NoCompletion path. Real adapters must always complete.
        RequestHandle::for_token(token)
    }
}

/// A scripted 200-OK response echoing the request URL as the final URL.
fn ok_response(request: &HttpRequest) -> Result<HttpResponse, HttpError> {
    Ok(HttpResponse::builder(
        StatusCode::OK,
        request.url().clone(),
        HttpVersion::Http1_1,
        BodyOutcome::Memory(Vec::new()),
    )
    .build())
}

/// A scripted 500 response — the deliberate break the harness must catch.
fn error_500_response(request: &HttpRequest) -> Result<HttpResponse, HttpError> {
    Ok(HttpResponse::builder(
        StatusCode::new(500),
        request.url().clone(),
        HttpVersion::Http1_1,
        BodyOutcome::Memory(Vec::new()),
    )
    .build())
}

/// A factory producing a fresh scripted [`MockHttp`] per row.
#[derive(Clone)]
pub struct MockFactory {
    script: Script,
}

impl MockFactory {
    /// A factory whose adapters run `script`.
    pub fn scripted(
        script: impl Fn(&HttpRequest) -> Option<Result<HttpResponse, HttpError>> + Send + Sync + 'static,
    ) -> Self {
        MockFactory {
            script: Arc::new(script),
        }
    }

    /// A correct mock: every request succeeds with `200 OK`. Passes the placeholder row.
    #[must_use]
    pub fn correct() -> Self {
        MockFactory::scripted(|req| Some(ok_response(req)))
    }

    /// A deliberately-broken mock: every request returns `500`. Fails the placeholder row — the
    /// fail-correctly demonstration.
    #[must_use]
    pub fn broken() -> Self {
        MockFactory::scripted(|req| Some(error_500_response(req)))
    }

    /// A silent mock: never delivers a completion. Exercises the harness's NoCompletion path.
    #[must_use]
    pub fn never_completes() -> Self {
        MockFactory::scripted(|_req| None)
    }
}

impl AdapterFactory for MockFactory {
    fn new_adapter(&self) -> Box<dyn Http> {
        Box::new(MockHttp {
            script: self.script.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::CompletionSink;
    use crate::request::Url;
    use std::sync::mpsc;
    use std::time::Duration;

    struct CollectSink(mpsc::Sender<Result<HttpResponse, HttpError>>);
    impl CompletionSink for CollectSink {
        fn complete(self: Box<Self>, outcome: Result<HttpResponse, HttpError>) {
            let _ = self.0.send(outcome);
        }
    }

    #[test]
    fn correct_mock_completes_with_200() {
        let adapter = MockFactory::correct().new_adapter();
        let url = Url::https("https://echo.test/").expect("valid url");
        let req =
            HttpRequest::builder(crate::request::Method::Get, url, Duration::from_secs(30)).build();
        let (tx, rx) = mpsc::channel();
        let _h = adapter.send(req, Box::new(CollectSink(tx)), None);
        let outcome = rx.recv_timeout(Duration::from_secs(1)).expect("completed");
        assert_eq!(outcome.expect("ok").status(), StatusCode::OK);
    }
}
