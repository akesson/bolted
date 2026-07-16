//! `sync-wire` — the hand-written as-if-generated IPC protocol (step 18, M2).
//!
//! Phase-1 doctrine: write the generated code by hand first. This crate is what `bolted-ffi-gen`
//! would one day emit for a wire target, and its friction log is that generator's requirements
//! document. Three properties are load-bearing:
//!
//! - **Values only.** Frames carry raw field values, keyed errors, ids and versions — never a
//!   validity judgement, never a constraint literal. The moment this crate would need one to
//!   function is step-18 kill criterion 3. Pinned by `tests/values_only.rs` from both sides.
//! - **Zero bolted dependencies.** The Swift client decodes the same protocol with `Codable` and
//!   cannot link `bolted-core`; the Rust protocol crate holding itself to the same constraint is
//!   what makes the two clients comparable.
//! - **A D27-style versioned envelope.** Every frame carries `v`; the version is checked before
//!   the body is parsed; an unknown version is a typed refusal, not a guess.
//!
//! Framing is newline-delimited compact JSON — a spike wants debuggability (`nc -U` shows the
//! whole conversation); the codec is swappable and its cost is measured (probe row D), not argued.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

/// Bumped on any frame-shape change. The daemon refuses other versions with
/// [`RefusalReason::UnknownVersion`] — parse-don't-validate at the process boundary.
pub const SCHEMA_VERSION: u32 = 1;

// =================================================================================================
// Wire data — raw values, keyed errors, ids, versions. Nothing else.
// =================================================================================================

/// A structured, localisable error: a stable key plus named params. The wire twin of the core's
/// keyed error data — owned strings because this side of the boundary has no `'static` tables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorWire {
    pub key: String,
    pub params: Vec<(String, String)>,
}

/// The vehicle's field names. As-if-generated: one variant per declared field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldName {
    Label,
    Folder,
    Interval,
    Paused,
}

/// The vehicle's declared checks. As-if-generated: one variant per `#[check(..)]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckName {
    FolderReachable,
}

/// A raw input value, exactly as a shell would send it: the vehicle has text raws and one bool
/// raw. Untagged — a JSON string and a JSON bool are already self-describing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RawWire {
    Text(String),
    Flag(bool),
}

/// A tier-2 rule violation as data: which rule, which fields it pins to, the keyed error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleWire {
    pub rule: String,
    pub pins: Vec<FieldName>,
    pub error: ErrorWire,
}

/// A full validation report: tier-1 keyed errors per field, tier-2 rule violations — including
/// the async check's pending/required/failed keys, which the core folds in itself. This is how
/// every judgement crosses the wire: as the core's own report, never re-derived here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportWire {
    pub field_errors: Vec<(FieldName, ErrorWire)>,
    pub rule_errors: Vec<RuleWire>,
}

impl ReportWire {
    pub fn is_ok(&self) -> bool {
        self.field_errors.is_empty() && self.rule_errors.is_empty()
    }
}

/// One field of a draft snapshot: the last attempt, the ancestor, value-based dirtiness, and the
/// incoming canonical value iff conflicted. Whether the attempt was *rejected* is not restated
/// here — it arrives as a keyed entry in [`DraftWire::report`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldWire {
    pub raw: Option<RawWire>,
    pub base: Option<RawWire>,
    pub dirty: bool,
    pub theirs: Option<RawWire>,
}

/// A draft snapshot. Fetched, never streamed: the daemon pushes a small tick and the client
/// fetches — the step-04 read-direct + version-tick pattern with one extra round-trip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DraftWire {
    pub draft: u64,
    pub label: FieldWire,
    pub folder: FieldWire,
    pub interval: FieldWire,
    pub paused: FieldWire,
    pub orphaned: bool,
    pub base_version: u64,
    pub report: ReportWire,
}

/// The canonical entity: always-valid values in raw form, plus the store version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalWire {
    pub version: u64,
    pub label: String,
    pub folder: String,
    pub interval: String,
    pub paused: bool,
}

/// One field of a stash: `{raw, base}`, both in raw form (C20 — no sync state, no verdict).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldStashWire {
    pub raw: Option<RawWire>,
    pub base: Option<RawWire>,
}

/// A draft flattened for survival across process death — including the *daemon's* (H6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StashWire {
    pub label: FieldStashWire,
    pub folder: FieldStashWire,
    pub interval: FieldStashWire,
    pub paused: FieldStashWire,
    pub base_version: u64,
    pub orphaned: bool,
}

/// Why a submit was refused — the store's typed refusal, verbatim as data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SubmitRefusalWire {
    Validation { report: ReportWire },
    Conflicted { fields: Vec<FieldName> },
    Orphaned,
    AlreadySubmitted,
}

// =================================================================================================
// Requests (client → daemon)
// =================================================================================================

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Request {
    /// Round-trip floor: framing + syscalls, no core work (probe row D1).
    Ping,
    Version,
    /// Store counts, for the disconnect-pruning probe (E2). Numbers, not judgements.
    Stats,
    Checkout,
    CanonicalSnapshot,
    DraftSnapshot {
        draft: u64,
    },
    TrySet {
        draft: u64,
        field: FieldName,
        value: RawWire,
    },
    Validate {
        draft: u64,
    },
    Resolve {
        draft: u64,
        field: FieldName,
        keep_mine: bool,
    },
    /// Start the named check. The unforgeable core token stays daemon-side; the response carries
    /// a wire-level correlation id — exactly how the FFI wrapper already treats `CheckToken`.
    BeginCheck {
        draft: u64,
        check: CheckName,
    },
    /// Settle a check: `ok: true` passes; `false` maps daemon-side to the check's DECLARED failed
    /// key. Verdicts cross boundaries as closed data, never as open error payloads — the FFI
    /// wrapper maps its verdict enum to the declared key the same way (core error keys are
    /// `'static` by design). A stale or unknown token settles nothing (`accepted: false`) —
    /// single-flight crossing the wire (B2).
    CompleteCheck {
        draft: u64,
        check: CheckName,
        token: u64,
        ok: bool,
    },
    Submit {
        draft: u64,
    },
    Close {
        draft: u64,
    },
    Stash {
        draft: u64,
    },
    Restore {
        stash: StashWire,
    },
    /// The session-less mutation (§9's demoted `command` verb, hand-written in the vehicle).
    TogglePaused,
}

// =================================================================================================
// Responses & pushes (daemon → client)
// =================================================================================================

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Response {
    Pong,
    Version { version: u64 },
    Stats { drafts: u64, rebasing: u64 },
    DraftId { draft: u64 },
    Canonical { canonical: Option<CanonicalWire> },
    Draft { snapshot: DraftWire },
    SetOutcome { error: Option<ErrorWire> },
    Report { report: ReportWire },
    CheckBegun { token: u64 },
    CheckSettled { accepted: bool },
    Submitted { version: u64 },
    SubmitRefused { refusal: SubmitRefusalWire },
    Resolved,
    Closed,
    Stashed { stash: StashWire },
    Toggled { paused: bool },
    ToggleRefused { refusal: ToggleRefusalWire },
}

/// Why the session-less mutation refused — the vehicle's `ToggleError`, verbatim as data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToggleRefusalWire {
    NoCanonical,
    Validation { report: ReportWire },
}

/// Unsolicited daemon → client notifications: small ticks, the client fetches (row C3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Push {
    CanonicalChanged { version: u64 },
    DraftRebased { draft: u64, base_version: u64 },
}

/// A typed connection-level refusal. Never a judgement about *values* — those are `SetOutcome` /
/// `Report` / `SubmitRefused` data. These are boundary failures: bad envelope, bad ownership.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalReason {
    UnknownVersion,
    MalformedFrame,
    /// The id was never issued, or is no longer live, on this daemon.
    UnknownDraft,
    /// The id is live but owned by a different connection (probe row B4 — record, don't harden).
    NotYourDraft,
    /// The raw's JSON type does not match the field's declared raw (a marshalling error, the
    /// wire twin of an FFI signature mismatch — not a validity judgement).
    RawTypeMismatch,
}

// =================================================================================================
// The envelope
// =================================================================================================

/// Client → daemon: one request line. `seq` correlates the response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClientFrame {
    pub v: u32,
    pub seq: u64,
    pub req: Request,
}

/// Daemon → client: one line — a correlated response, an unsolicited push, or a typed refusal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServerFrame {
    Response {
        re: u64,
        resp: Box<Response>,
    },
    Push {
        push: Push,
    },
    Refused {
        re: Option<u64>,
        reason: RefusalReason,
    },
}

/// Daemon → client envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerEnvelope {
    pub v: u32,
    #[serde(flatten)]
    pub frame: ServerFrame,
}

/// The version-first probe: decode ONLY `v`, so an unknown version is refused before any body
/// shape is assumed (parse-don't-validate, D27).
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct VersionProbe {
    pub v: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WireError {
    Json(String),
    UnknownVersion { got: u32 },
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WireError::Json(e) => write!(f, "wire json error: {e}"),
            WireError::UnknownVersion { got } => {
                write!(
                    f,
                    "unknown schema version {got} (expected {SCHEMA_VERSION})"
                )
            }
        }
    }
}

impl std::error::Error for WireError {}

/// Encode one frame as one compact JSON line (no trailing newline — the writer owns framing).
pub fn encode<T: Serialize>(frame: &T) -> Result<String, WireError> {
    serde_json::to_string(frame).map_err(|e| WireError::Json(e.to_string()))
}

/// Check the envelope version of a received line, before decoding any body.
pub fn probe_version(line: &str) -> Result<(), WireError> {
    let probe: VersionProbe =
        serde_json::from_str(line).map_err(|e| WireError::Json(e.to_string()))?;
    if probe.v != SCHEMA_VERSION {
        return Err(WireError::UnknownVersion { got: probe.v });
    }
    Ok(())
}

/// Decode a version-checked line into a typed frame.
pub fn decode<T: serde::de::DeserializeOwned>(line: &str) -> Result<T, WireError> {
    probe_version(line)?;
    serde_json::from_str(line).map_err(|e| WireError::Json(e.to_string()))
}

// =================================================================================================
// The blocking client (Rust probe side; the Swift client mirrors this in ~a screen of Codable)
// =================================================================================================

#[derive(Debug)]
pub enum ClientError {
    Io(std::io::Error),
    Wire(WireError),
    /// The daemon refused the frame (typed).
    Refused(RefusalReason),
    /// The connection closed while a response was outstanding.
    Disconnected,
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Io(e) => write!(f, "client io: {e}"),
            ClientError::Wire(e) => write!(f, "client wire: {e}"),
            ClientError::Refused(r) => write!(f, "refused: {r:?}"),
            ClientError::Disconnected => write!(f, "daemon disconnected"),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<std::io::Error> for ClientError {
    fn from(e: std::io::Error) -> Self {
        ClientError::Io(e)
    }
}

impl From<WireError> for ClientError {
    fn from(e: WireError) -> Self {
        ClientError::Wire(e)
    }
}

/// A blocking probe client: request/response with pushes interleaved on the same stream. Pushes
/// that arrive while a response is awaited are queued, never dropped.
pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    seq: u64,
    pushes: VecDeque<Push>,
}

impl Client {
    pub fn connect(path: &Path) -> Result<Client, ClientError> {
        let stream = UnixStream::connect(path)?;
        // A probe must fail loudly, not hang: every read is bounded.
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        let writer = stream.try_clone()?;
        Ok(Client {
            reader: BufReader::new(stream),
            writer,
            seq: 0,
            pushes: VecDeque::new(),
        })
    }

    fn read_frame(&mut self) -> Result<ServerEnvelope, ClientError> {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line)?;
        if n == 0 {
            return Err(ClientError::Disconnected);
        }
        Ok(decode(&line)?)
    }

    /// Send `req`, return its response. Pushes arriving first are queued for [`Self::take_pushes`].
    pub fn request(&mut self, req: Request) -> Result<Response, ClientError> {
        self.seq += 1;
        let seq = self.seq;
        let line = encode(&ClientFrame {
            v: SCHEMA_VERSION,
            seq,
            req,
        })?;
        self.writer.write_all(line.as_bytes())?;
        self.writer.write_all(b"\n")?;
        loop {
            match self.read_frame()?.frame {
                ServerFrame::Response { re, resp } if re == seq => return Ok(*resp),
                ServerFrame::Response { .. } => continue, // a stale response to a lost request
                ServerFrame::Push { push } => self.pushes.push_back(push),
                ServerFrame::Refused { re, reason } if re == Some(seq) || re.is_none() => {
                    return Err(ClientError::Refused(reason));
                }
                ServerFrame::Refused { .. } => continue,
            }
        }
    }

    /// Drain the pushes queued so far without touching the socket.
    pub fn take_pushes(&mut self) -> Vec<Push> {
        self.pushes.drain(..).collect()
    }

    /// Block (bounded by the read timeout) until one push arrives, and return it.
    pub fn wait_push(&mut self) -> Result<Push, ClientError> {
        if let Some(p) = self.pushes.pop_front() {
            return Ok(p);
        }
        loop {
            match self.read_frame()?.frame {
                ServerFrame::Push { push } => return Ok(push),
                // A response with no request outstanding is a protocol error on our side; a probe
                // records it as a refusal-shaped failure rather than guessing.
                ServerFrame::Response { .. } => continue,
                ServerFrame::Refused { reason, .. } => return Err(ClientError::Refused(reason)),
            }
        }
    }

    /// Send a raw line (test hook for malformed/unknown-version frames) and read one reply.
    pub fn send_raw(&mut self, line: &str) -> Result<ServerEnvelope, ClientError> {
        self.writer.write_all(line.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.read_frame()
    }
}
