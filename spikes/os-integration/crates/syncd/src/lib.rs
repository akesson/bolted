//! `syncd` — the daemon: one process owning one `Store<SyncSettingsDraft>` that other processes
//! attach to (step 18, M2; §9 question 1's "daemon-owned" arm, being priced).
//!
//! Deliberately boring concurrency: a blocking accept loop, a thread per connection, and ONE
//! `Mutex` around everything shared — the FFI shells' proven shape. The single discipline that
//! matters is inherited from step 02 via D16: **never write to a socket while holding the store
//! lock.** The store returns its rebase fan-out as data (`Vec<DraftId>`), so push frames are
//! collected under the lock and flushed after it drops — the same two-phase move the FFI wrapper
//! makes, with `write_all` where the stream producer was.
//!
//! No FFI, no tokio (per the step doc: an async-runtime choice is a design decision this spike
//! must not smuggle in). Connection-scoped draft ownership: every draft a connection checks out
//! or restores is closed when that connection dies — C18's `close()` duty at process scope,
//! and the strictest cleanup policy (friction with it is design-pass input).

#![forbid(unsafe_code)]

use bolted_core::{
    CheckToken, Checked, Draft, DraftId, DraftStatus, ErrorData, Field, FieldStash, Stashable,
    Store, SubmitError, Value,
};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use sync_settings::{
    SyncSettings, SyncSettingsCheck, SyncSettingsDraft, SyncSettingsField, SyncSettingsStash,
    SyncSettingsStore, ToggleError, seed, toggle_paused,
};
use sync_wire as wire;
use sync_wire::{
    CanonicalWire, ClientFrame, DraftWire, ErrorWire, FieldName, FieldStashWire, FieldWire, Push,
    RawWire, RefusalReason, ReportWire, Request, Response, RuleWire, SCHEMA_VERSION,
    ServerEnvelope, ServerFrame, StashWire, SubmitRefusalWire, ToggleRefusalWire,
};

// =================================================================================================
// Shared state
// =================================================================================================

type Writer = Arc<Mutex<UnixStream>>;
/// Encoded lines to write AFTER the store lock is dropped (never emit under the lock — D16).
type Emits = Vec<(Writer, String)>;

/// One connection's registry: its writer half, the drafts it owns, and the daemon-side core
/// tokens for its in-flight checks. `CheckToken` is deliberately unforgeable, so it cannot cross
/// the wire; the connection layer keeps it and hands the client a correlation id — exactly what
/// the FFI wrapper does (recorded as a wire-generator requirement).
struct Conn {
    writer: Writer,
    /// wire id (`DraftId::as_u64`) → the store's unforgeable id.
    drafts: BTreeMap<u64, DraftId>,
    /// wire token → (wire draft id, the core token the store issued).
    tokens: BTreeMap<u64, (u64, CheckToken)>,
}

/// Everything the one mutex protects.
pub struct Shared {
    store: SyncSettingsStore,
    conns: BTreeMap<u64, Conn>,
    next_conn: u64,
    next_token: u64,
}

/// A daemon state seeded with the vehicle's boot canonical (not persisted — the gap is scope,
/// recorded in the report).
pub fn new_shared() -> Arc<Mutex<Shared>> {
    Arc::new(Mutex::new(Shared {
        store: Store::new(seed()),
        conns: BTreeMap::new(),
        next_conn: 0,
        next_token: 0,
    }))
}

/// Poison-safe locking (no `unwrap`/`expect`/`panic!` in library code, per CLAUDE.md).
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

// =================================================================================================
// Serving
// =================================================================================================

/// Accept loop: one thread per connection. Returns when the listener fails (e.g. it was closed).
pub fn serve(listener: UnixListener, shared: Arc<Mutex<Shared>>) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { break };
        let shared = Arc::clone(&shared);
        thread::spawn(move || handle_conn(stream, shared));
    }
}

fn handle_conn(stream: UnixStream, shared: Arc<Mutex<Shared>>) {
    let Ok(write_half) = stream.try_clone() else {
        return;
    };
    let writer: Writer = Arc::new(Mutex::new(write_half));

    let conn_id = {
        let mut g = lock(&shared);
        let id = g.next_conn;
        g.next_conn += 1;
        g.conns.insert(
            id,
            Conn {
                writer: Arc::clone(&writer),
                drafts: BTreeMap::new(),
                tokens: BTreeMap::new(),
            },
        );
        id
    };

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break, // EOF or a dead peer — a client kill -9 looks exactly like this
            Ok(_) => {}
        }
        if line.trim().is_empty() {
            continue;
        }
        let (reply, emits) = handle_line(&shared, conn_id, &line);
        flush(vec![(Arc::clone(&writer), reply)]);
        flush(emits);
    }

    // Disconnect pruning (E2): the strictest policy — every draft this connection owned closes.
    let mut g = lock(&shared);
    if let Some(conn) = g.conns.remove(&conn_id) {
        for id in conn.drafts.into_values() {
            g.store.close(id);
        }
    }
}

fn flush(emits: Emits) {
    for (writer, line) in emits {
        let mut w = lock(&writer);
        // A failed write means the peer is going or gone; its reader thread cleans up.
        let _ = w.write_all(line.as_bytes());
        let _ = w.write_all(b"\n");
    }
}

fn envelope(frame: ServerFrame) -> String {
    let env = ServerEnvelope {
        v: SCHEMA_VERSION,
        frame,
    };
    // Encoding our own frames cannot realistically fail; degrade to a refusal literal if it does.
    wire::encode(&env).unwrap_or_else(|_| {
        format!("{{\"v\":{SCHEMA_VERSION},\"kind\":\"refused\",\"re\":null,\"reason\":\"malformed_frame\"}}")
    })
}

/// Parse one request line and run it. Returns the reply line plus any push lines for other
/// connections (all flushed by the caller, outside the lock).
fn handle_line(shared: &Arc<Mutex<Shared>>, conn_id: u64, line: &str) -> (String, Emits) {
    if let Err(e) = wire::probe_version(line) {
        let reason = match e {
            wire::WireError::UnknownVersion { .. } => RefusalReason::UnknownVersion,
            wire::WireError::Json(_) => RefusalReason::MalformedFrame,
        };
        return (
            envelope(ServerFrame::Refused { re: None, reason }),
            Vec::new(),
        );
    }
    let frame: ClientFrame = match wire::decode(line) {
        Ok(f) => f,
        Err(_) => {
            return (
                envelope(ServerFrame::Refused {
                    re: None,
                    reason: RefusalReason::MalformedFrame,
                }),
                Vec::new(),
            );
        }
    };

    let mut g = lock(shared);
    let (outcome, emits) = handle_request(&mut g, conn_id, frame.req);
    drop(g);

    let reply = match outcome {
        Ok(resp) => ServerFrame::Response {
            re: frame.seq,
            resp: Box::new(resp),
        },
        Err(reason) => ServerFrame::Refused {
            re: Some(frame.seq),
            reason,
        },
    };
    (envelope(reply), emits)
}

// =================================================================================================
// Request handling (under the one lock; emissions returned as data)
// =================================================================================================

/// Resolve a wire draft id against THIS connection's registry. An id owned by another connection
/// is a distinct typed refusal (probe row B4 — record the shape, don't harden it).
fn resolve(g: &Shared, conn_id: u64, wire_id: u64) -> Result<DraftId, RefusalReason> {
    if let Some(conn) = g.conns.get(&conn_id)
        && let Some(id) = conn.drafts.get(&wire_id)
    {
        return Ok(*id);
    }
    let owned_elsewhere = g
        .conns
        .iter()
        .any(|(cid, c)| *cid != conn_id && c.drafts.contains_key(&wire_id));
    Err(if owned_elsewhere {
        RefusalReason::NotYourDraft
    } else {
        RefusalReason::UnknownDraft
    })
}

/// Push frames for a canonical change: one version tick per connection, plus a rebased tick to
/// each rebased draft's owner. Built under the lock, flushed after it.
fn broadcast(g: &Shared, version: u64, rebased: &[DraftId]) -> Emits {
    let mut emits = Vec::new();
    let tick = envelope(ServerFrame::Push {
        push: Push::CanonicalChanged { version },
    });
    for conn in g.conns.values() {
        emits.push((Arc::clone(&conn.writer), tick.clone()));
        for id in rebased {
            if conn.drafts.contains_key(&id.as_u64()) {
                emits.push((
                    Arc::clone(&conn.writer),
                    envelope(ServerFrame::Push {
                        push: Push::DraftRebased {
                            draft: id.as_u64(),
                            base_version: version,
                        },
                    }),
                ));
            }
        }
    }
    emits
}

fn register_draft(g: &mut Shared, conn_id: u64, id: DraftId) {
    if let Some(conn) = g.conns.get_mut(&conn_id) {
        conn.drafts.insert(id.as_u64(), id);
    }
}

fn handle_request(
    g: &mut Shared,
    conn_id: u64,
    req: Request,
) -> (Result<Response, RefusalReason>, Emits) {
    let no_emit = Vec::new();
    match req {
        Request::Ping => (Ok(Response::Pong), no_emit),
        Request::Version => (
            Ok(Response::Version {
                version: g.store.version(),
            }),
            no_emit,
        ),
        Request::Stats => (
            Ok(Response::Stats {
                drafts: g.store.draft_count() as u64,
                rebasing: g.store.rebasing_draft_count() as u64,
            }),
            no_emit,
        ),
        Request::Checkout => {
            let id = g.store.checkout();
            register_draft(g, conn_id, id);
            (Ok(Response::DraftId { draft: id.as_u64() }), no_emit)
        }
        Request::CanonicalSnapshot => {
            let version = g.store.version();
            let canonical = g.store.canonical().map(|e| canonical_wire(e, version));
            (Ok(Response::Canonical { canonical }), no_emit)
        }
        Request::DraftSnapshot { draft } => match resolve(g, conn_id, draft) {
            Ok(id) => match g.store.draft(id) {
                Some(d) => (
                    Ok(Response::Draft {
                        snapshot: draft_wire(draft, d),
                    }),
                    no_emit,
                ),
                None => (Err(RefusalReason::UnknownDraft), no_emit),
            },
            Err(r) => (Err(r), no_emit),
        },
        Request::TrySet {
            draft,
            field,
            value,
        } => match resolve(g, conn_id, draft) {
            Ok(id) => match g.store.draft_mut(id) {
                Some(d) => match try_set(d, field, value) {
                    Ok(error) => (Ok(Response::SetOutcome { error }), no_emit),
                    Err(reason) => (Err(reason), no_emit),
                },
                None => (Err(RefusalReason::UnknownDraft), no_emit),
            },
            Err(r) => (Err(r), no_emit),
        },
        Request::Validate { draft } => match resolve(g, conn_id, draft) {
            Ok(id) => match g.store.draft(id) {
                Some(d) => (
                    Ok(Response::Report {
                        report: report_wire(&d.validate()),
                    }),
                    no_emit,
                ),
                None => (Err(RefusalReason::UnknownDraft), no_emit),
            },
            Err(r) => (Err(r), no_emit),
        },
        Request::Resolve {
            draft,
            field,
            keep_mine,
        } => match resolve(g, conn_id, draft) {
            Ok(id) => match g.store.draft_mut(id) {
                Some(d) => {
                    let f = core_field(field);
                    if keep_mine {
                        d.resolve_keep_mine(f);
                    } else {
                        d.resolve_take_theirs(f);
                    }
                    (Ok(Response::Resolved), no_emit)
                }
                None => (Err(RefusalReason::UnknownDraft), no_emit),
            },
            Err(r) => (Err(r), no_emit),
        },
        Request::BeginCheck { draft, check } => match resolve(g, conn_id, draft) {
            Ok(id) => match g.store.draft_mut(id) {
                Some(d) => {
                    let core_token = d.begin_check(core_check(check));
                    let token = g.next_token;
                    g.next_token += 1;
                    if let Some(conn) = g.conns.get_mut(&conn_id) {
                        conn.tokens.insert(token, (draft, core_token));
                    }
                    (Ok(Response::CheckBegun { token }), no_emit)
                }
                None => (Err(RefusalReason::UnknownDraft), no_emit),
            },
            Err(r) => (Err(r), no_emit),
        },
        Request::CompleteCheck {
            draft,
            check,
            token,
            ok,
        } => match resolve(g, conn_id, draft) {
            Ok(id) => {
                let entry = g
                    .conns
                    .get_mut(&conn_id)
                    .and_then(|conn| conn.tokens.remove(&token));
                let accepted = match entry {
                    Some((token_draft, core_token)) if token_draft == draft => {
                        // The verdict is closed data: a failure maps to the check's DECLARED
                        // failed key, exactly as the FFI wrapper maps its verdict enum.
                        let verdict = if ok {
                            Ok(())
                        } else {
                            Err(ErrorData::new("folder_unreachable"))
                        };
                        match g.store.draft_mut(id) {
                            Some(d) => d.complete_check(core_check(check), core_token, verdict),
                            None => false,
                        }
                    }
                    // Unknown, stale, or another draft's token settles nothing (B2's semantics).
                    _ => false,
                };
                (Ok(Response::CheckSettled { accepted }), no_emit)
            }
            Err(r) => (Err(r), no_emit),
        },
        Request::Submit { draft } => match resolve(g, conn_id, draft) {
            Ok(id) => match g.store.submit(id) {
                Ok(rebased) => {
                    if let Some(conn) = g.conns.get_mut(&conn_id) {
                        conn.drafts.remove(&draft);
                    }
                    let version = g.store.version();
                    let emits = broadcast(g, version, &rebased);
                    (Ok(Response::Submitted { version }), emits)
                }
                Err(e) => (
                    Ok(Response::SubmitRefused {
                        refusal: submit_refusal_wire(e),
                    }),
                    no_emit,
                ),
            },
            Err(r) => (Err(r), no_emit),
        },
        Request::Close { draft } => match resolve(g, conn_id, draft) {
            Ok(id) => {
                g.store.close(id);
                if let Some(conn) = g.conns.get_mut(&conn_id) {
                    conn.drafts.remove(&draft);
                }
                (Ok(Response::Closed), no_emit)
            }
            Err(r) => (Err(r), no_emit),
        },
        Request::Stash { draft } => match resolve(g, conn_id, draft) {
            Ok(id) => match g.store.draft(id) {
                Some(d) => (
                    Ok(Response::Stashed {
                        stash: stash_wire(&d.stash()),
                    }),
                    no_emit,
                ),
                None => (Err(RefusalReason::UnknownDraft), no_emit),
            },
            Err(r) => (Err(r), no_emit),
        },
        Request::Restore { stash } => match stash_from_wire(&stash) {
            Some(core_stash) => {
                let id = g.store.restore(&core_stash);
                register_draft(g, conn_id, id);
                (Ok(Response::DraftId { draft: id.as_u64() }), no_emit)
            }
            None => (Err(RefusalReason::RawTypeMismatch), no_emit),
        },
        Request::TogglePaused => match toggle_paused(&mut g.store) {
            Ok((paused, rebased)) => {
                let version = g.store.version();
                let emits = broadcast(g, version, &rebased);
                (Ok(Response::Toggled { paused }), emits)
            }
            Err(e) => (
                Ok(Response::ToggleRefused {
                    refusal: match *e {
                        ToggleError::NoCanonical => ToggleRefusalWire::NoCanonical,
                        ToggleError::Validation(report) => ToggleRefusalWire::Validation {
                            report: report_wire(&report),
                        },
                    },
                }),
                no_emit,
            ),
        },
    }
}

/// One setter dispatch: the (field, raw-type) table a generator would emit. A mismatched raw is a
/// marshalling refusal (the wire twin of an FFI signature mismatch), not a validity judgement.
fn try_set(
    d: &mut SyncSettingsDraft,
    field: FieldName,
    value: RawWire,
) -> Result<Option<ErrorWire>, RefusalReason> {
    match (field, value) {
        (FieldName::Label, RawWire::Text(s)) => Ok(set_outcome(d.try_set_label(s))),
        (FieldName::Folder, RawWire::Text(s)) => Ok(set_outcome(d.try_set_folder(s))),
        (FieldName::Interval, RawWire::Text(s)) => Ok(set_outcome(d.try_set_interval(s))),
        (FieldName::Paused, RawWire::Flag(b)) => {
            let _infallible = d.try_set_paused(b);
            Ok(None)
        }
        _ => Err(RefusalReason::RawTypeMismatch),
    }
}

fn set_outcome<E: Into<ErrorData>>(r: Result<(), E>) -> Option<ErrorWire> {
    r.err().map(|e| error_wire(&e.into()))
}

// =================================================================================================
// Projections: core state → wire data. All judgement content travels as the core's own keyed
// report; these functions flatten values and never decide anything.
// =================================================================================================

fn core_field(f: FieldName) -> SyncSettingsField {
    match f {
        FieldName::Label => SyncSettingsField::Label,
        FieldName::Folder => SyncSettingsField::Folder,
        FieldName::Interval => SyncSettingsField::Interval,
        FieldName::Paused => SyncSettingsField::Paused,
    }
}

fn field_name(f: SyncSettingsField) -> FieldName {
    match f {
        SyncSettingsField::Label => FieldName::Label,
        SyncSettingsField::Folder => FieldName::Folder,
        SyncSettingsField::Interval => FieldName::Interval,
        SyncSettingsField::Paused => FieldName::Paused,
    }
}

fn core_check(c: wire::CheckName) -> SyncSettingsCheck {
    match c {
        wire::CheckName::FolderReachable => SyncSettingsCheck::FolderReachable,
    }
}

fn error_wire(e: &ErrorData) -> ErrorWire {
    ErrorWire {
        key: e.key.to_string(),
        params: e
            .params
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
    }
}

fn report_wire(r: &bolted_core::ValidationReport<SyncSettingsField>) -> ReportWire {
    ReportWire {
        field_errors: r
            .field_errors
            .iter()
            .map(|(f, e)| (field_name(*f), error_wire(e)))
            .collect(),
        rule_errors: r
            .rule_errors
            .iter()
            .map(|v| RuleWire {
                rule: v.rule.to_string(),
                pins: v.pins.iter().map(|f| field_name(*f)).collect(),
                error: error_wire(&v.error),
            })
            .collect(),
    }
}

/// Flatten one field via its own stash projection (`{raw, base}` — C20's shape), plus the two
/// value-derived bits a fetching client needs. No `match` over any core judgement enum.
fn text_field<V: Value<Raw = String>>(f: &Field<V>) -> FieldWire {
    let FieldStash { raw, base } = f.stash();
    FieldWire {
        raw: raw.map(RawWire::Text),
        base: base.map(RawWire::Text),
        dirty: f.is_dirty(),
        theirs: f.theirs().map(|v| RawWire::Text(v.clone().into_raw())),
    }
}

fn flag_field<V: Value<Raw = bool>>(f: &Field<V>) -> FieldWire {
    let FieldStash { raw, base } = f.stash();
    FieldWire {
        raw: raw.map(RawWire::Flag),
        base: base.map(RawWire::Flag),
        dirty: f.is_dirty(),
        theirs: f.theirs().map(|v| RawWire::Flag(v.clone().into_raw())),
    }
}

fn draft_wire(wire_id: u64, d: &SyncSettingsDraft) -> DraftWire {
    DraftWire {
        draft: wire_id,
        label: text_field(&d.label),
        folder: text_field(&d.folder),
        interval: text_field(&d.interval),
        paused: flag_field(&d.paused),
        orphaned: matches!(d.status(), DraftStatus::Orphaned),
        base_version: d.base_version(),
        report: report_wire(&d.validate()),
    }
}

fn canonical_wire(e: &SyncSettings, version: u64) -> CanonicalWire {
    CanonicalWire {
        version,
        label: e.label.as_str().to_string(),
        folder: e.folder.as_str().to_string(),
        interval: e.interval.as_str().to_string(),
        paused: e.paused.is_on(),
    }
}

fn text_stash(s: &FieldStash<String>) -> FieldStashWire {
    FieldStashWire {
        raw: s.raw.clone().map(RawWire::Text),
        base: s.base.clone().map(RawWire::Text),
    }
}

fn flag_stash(s: &FieldStash<bool>) -> FieldStashWire {
    FieldStashWire {
        raw: s.raw.map(RawWire::Flag),
        base: s.base.map(RawWire::Flag),
    }
}

fn stash_wire(s: &SyncSettingsStash) -> StashWire {
    StashWire {
        label: text_stash(&s.label),
        folder: text_stash(&s.folder),
        interval: text_stash(&s.interval),
        paused: flag_stash(&s.paused),
        base_version: s.base_version,
        orphaned: s.orphaned,
    }
}

fn text_stash_back(w: &FieldStashWire) -> Option<FieldStash<String>> {
    let get = |r: &Option<RawWire>| match r {
        None => Some(None),
        Some(RawWire::Text(s)) => Some(Some(s.clone())),
        Some(RawWire::Flag(_)) => None,
    };
    Some(FieldStash {
        raw: get(&w.raw)?,
        base: get(&w.base)?,
    })
}

fn flag_stash_back(w: &FieldStashWire) -> Option<FieldStash<bool>> {
    let get = |r: &Option<RawWire>| match r {
        None => Some(None),
        Some(RawWire::Flag(b)) => Some(Some(*b)),
        Some(RawWire::Text(_)) => None,
    };
    Some(FieldStash {
        raw: get(&w.raw)?,
        base: get(&w.base)?,
    })
}

/// Wire stash → core stash. `None` on a raw-type mismatch: the stash is the first UNTRUSTED input
/// in the system (the core already treats it so — a raw that stopped parsing lands `Invalid`, a
/// base that stopped parsing degrades to create-flow), and the wire layer adds only the JSON-type
/// gate the core's typed `Raw` makes unrepresentable in-process.
fn stash_from_wire(w: &StashWire) -> Option<SyncSettingsStash> {
    Some(SyncSettingsStash {
        label: text_stash_back(&w.label)?,
        folder: text_stash_back(&w.folder)?,
        interval: text_stash_back(&w.interval)?,
        paused: flag_stash_back(&w.paused)?,
        base_version: w.base_version,
        orphaned: w.orphaned,
    })
}

fn submit_refusal_wire(e: SubmitError<SyncSettingsField>) -> SubmitRefusalWire {
    match e {
        SubmitError::Validation(report) => SubmitRefusalWire::Validation {
            report: report_wire(&report),
        },
        SubmitError::Conflicted { fields } => SubmitRefusalWire::Conflicted {
            fields: fields.into_iter().map(field_name).collect(),
        },
        SubmitError::Orphaned => SubmitRefusalWire::Orphaned,
        SubmitError::AlreadySubmitted => SubmitRefusalWire::AlreadySubmitted,
    }
}
