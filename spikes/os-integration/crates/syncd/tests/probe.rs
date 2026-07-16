//! M2 — the contract over IPC, on a real Unix socket (probe rows B, E, and F's in-process
//! variant; the launchd/kill -9 halves of A and F are M4). Every test starts its own daemon on
//! its own socket path; the daemon threads die with the test process.

use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use sync_wire::{
    CheckName, Client, ClientError, FieldName, Push, RawWire, RefusalReason, Request, Response,
    ServerFrame, SubmitRefusalWire,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn start_daemon() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "syncd-probe-{}-{}.sock",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).expect("bind test socket");
    let shared = syncd::new_shared();
    std::thread::spawn(move || syncd::serve(listener, shared));
    path
}

fn checkout(c: &mut Client) -> u64 {
    match c.request(Request::Checkout).expect("checkout") {
        Response::DraftId { draft } => draft,
        other => panic!("expected DraftId, got {other:?}"),
    }
}

fn try_set(
    c: &mut Client,
    draft: u64,
    field: FieldName,
    value: RawWire,
) -> Option<sync_wire::ErrorWire> {
    match c
        .request(Request::TrySet {
            draft,
            field,
            value,
        })
        .expect("try_set")
    {
        Response::SetOutcome { error } => error,
        other => panic!("expected SetOutcome, got {other:?}"),
    }
}

fn set_text(c: &mut Client, draft: u64, field: FieldName, s: &str) -> Option<sync_wire::ErrorWire> {
    try_set(c, draft, field, RawWire::Text(s.to_string()))
}

fn validate(c: &mut Client, draft: u64) -> sync_wire::ReportWire {
    match c.request(Request::Validate { draft }).expect("validate") {
        Response::Report { report } => report,
        other => panic!("expected Report, got {other:?}"),
    }
}

fn snapshot(c: &mut Client, draft: u64) -> sync_wire::DraftWire {
    match c
        .request(Request::DraftSnapshot { draft })
        .expect("snapshot")
    {
        Response::Draft { snapshot } => snapshot,
        other => panic!("expected Draft, got {other:?}"),
    }
}

fn rule_keys(report: &sync_wire::ReportWire) -> Vec<&str> {
    report
        .rule_errors
        .iter()
        .map(|r| r.error.key.as_str())
        .collect()
}

// =================================================================================================
// B1 — the full draft cycle, remotely
// =================================================================================================

#[test]
fn b1_full_draft_cycle_over_the_socket() {
    let path = start_daemon();
    let mut c = Client::connect(&path).expect("connect");

    assert!(matches!(
        c.request(Request::Ping).expect("ping"),
        Response::Pong
    ));
    assert!(matches!(
        c.request(Request::Version).expect("version"),
        Response::Version { version: 0 }
    ));

    let draft = checkout(&mut c);

    // A rejected input comes back as a keyed error with STRUCTURED params, intact through the
    // envelope — the same data an in-process shell gets.
    let err = set_text(&mut c, draft, FieldName::Label, &"x".repeat(31)).expect("too long");
    assert_eq!(err.key, "too_long");
    assert_eq!(
        err.params,
        vec![
            ("max".to_string(), "30".to_string()),
            ("actual".to_string(), "31".to_string()),
        ]
    );
    let err = set_text(&mut c, draft, FieldName::Folder, "relative/path").expect("not absolute");
    assert_eq!(err.key, "not_absolute");

    // Valid edits; the tier-2 rule fires across the wire, pinned to the interval.
    assert!(set_text(&mut c, draft, FieldName::Label, "Photos").is_none());
    assert!(set_text(&mut c, draft, FieldName::Folder, "/Volumes/NAS/Photos").is_none());
    assert!(set_text(&mut c, draft, FieldName::Interval, "5").is_none());
    let report = validate(&mut c, draft);
    let rule = report
        .rule_errors
        .iter()
        .find(|r| r.rule == "network_volume_interval")
        .expect("tier-2 rule crossed the wire");
    assert_eq!(rule.pins, vec![FieldName::Interval]);
    assert_eq!(rule.error.key, "network_interval_too_fast");

    assert!(set_text(&mut c, draft, FieldName::Interval, "30").is_none());

    // The dirty folder's check gates the submit until driven (C16 as data, then B2 in full).
    assert!(rule_keys(&validate(&mut c, draft)).contains(&"folder_check_required"));
    let token = match c
        .request(Request::BeginCheck {
            draft,
            check: CheckName::FolderReachable,
        })
        .expect("begin")
    {
        Response::CheckBegun { token } => token,
        other => panic!("expected CheckBegun, got {other:?}"),
    };
    assert!(matches!(
        c.request(Request::CompleteCheck {
            draft,
            check: CheckName::FolderReachable,
            token,
            ok: true,
        })
        .expect("complete"),
        Response::CheckSettled { accepted: true }
    ));
    assert!(validate(&mut c, draft).is_ok());

    match c.request(Request::Submit { draft }).expect("submit") {
        Response::Submitted { version } => assert_eq!(version, 1),
        other => panic!("expected Submitted, got {other:?}"),
    }
    match c.request(Request::CanonicalSnapshot).expect("canonical") {
        Response::Canonical {
            canonical: Some(canon),
        } => {
            assert_eq!(canon.version, 1);
            assert_eq!(canon.label, "Photos");
            assert_eq!(canon.folder, "/Volumes/NAS/Photos");
        }
        other => panic!("expected canonical, got {other:?}"),
    }
}

// =================================================================================================
// B2 — the async check remotely: single-flight holds when the driver is another process
// =================================================================================================

#[test]
fn b2_single_flight_check_across_the_wire() {
    let path = start_daemon();
    let mut c = Client::connect(&path).expect("connect");
    let draft = checkout(&mut c);
    assert!(set_text(&mut c, draft, FieldName::Folder, "/Users/Shared/Other").is_none());

    // C16 reaches the client as data.
    assert!(rule_keys(&validate(&mut c, draft)).contains(&"folder_check_required"));

    let begin = |c: &mut Client| match c
        .request(Request::BeginCheck {
            draft,
            check: CheckName::FolderReachable,
        })
        .expect("begin")
    {
        Response::CheckBegun { token } => token,
        other => panic!("expected CheckBegun, got {other:?}"),
    };
    let complete = |c: &mut Client, token: u64, ok: bool| match c
        .request(Request::CompleteCheck {
            draft,
            check: CheckName::FolderReachable,
            token,
            ok,
        })
        .expect("complete")
    {
        Response::CheckSettled { accepted } => accepted,
        other => panic!("expected CheckSettled, got {other:?}"),
    };

    // Pending is visible as data while in flight.
    let stale = begin(&mut c);
    assert!(rule_keys(&validate(&mut c, draft)).contains(&"folder_check_pending"));

    // The newest begin supersedes; the stale completion is discarded (C10 over IPC).
    let fresh = begin(&mut c);
    assert!(
        !complete(&mut c, stale, true),
        "stale token must settle nothing"
    );
    assert!(complete(&mut c, fresh, true));
    assert!(validate(&mut c, draft).is_ok());

    // A token the daemon never issued settles nothing either.
    let ghost = begin(&mut c);
    assert!(!complete(&mut c, 99_999, true));
    // ...and the real one still lands, now with a failing verdict mapped to the DECLARED key.
    assert!(complete(&mut c, ghost, false));
    assert!(rule_keys(&validate(&mut c, draft)).contains(&"folder_unreachable"));
}

// =================================================================================================
// B3 — the session-less mutation, with its fan-out observed by another process
// =================================================================================================

#[test]
fn b3_toggle_paused_fans_out_to_the_other_client() {
    let path = start_daemon();
    let mut a = Client::connect(&path).expect("connect a");
    let mut b = Client::connect(&path).expect("connect b");
    let b_draft = checkout(&mut b);

    match a.request(Request::TogglePaused).expect("toggle") {
        Response::Toggled { paused } => assert!(paused),
        other => panic!("expected Toggled, got {other:?}"),
    }

    // B gets the small tick, then its per-draft rebase tick — and fetches (tick-then-fetch).
    assert_eq!(
        b.wait_push().expect("tick"),
        Push::CanonicalChanged { version: 1 }
    );
    assert_eq!(
        b.wait_push().expect("rebased"),
        Push::DraftRebased {
            draft: b_draft,
            base_version: 1
        }
    );
    let snap = snapshot(&mut b, b_draft);
    assert_eq!(
        snap.paused.raw,
        Some(RawWire::Flag(true)),
        "clean field adopted theirs"
    );
    assert!(!snap.paused.dirty);
    assert_eq!(snap.base_version, 1);
}

// =================================================================================================
// B4 — draft-id hygiene across connections (record the shape, don't harden)
// =================================================================================================

#[test]
fn b4_foreign_and_forged_draft_ids_get_typed_refusals() {
    let path = start_daemon();
    let mut a = Client::connect(&path).expect("connect a");
    let mut b = Client::connect(&path).expect("connect b");
    let a_draft = checkout(&mut a);

    // Another connection's live draft: a distinct typed refusal (the D23 shape, security flavor).
    match b.request(Request::DraftSnapshot { draft: a_draft }) {
        Err(ClientError::Refused(RefusalReason::NotYourDraft)) => {}
        other => panic!("expected NotYourDraft, got {other:?}"),
    }
    // An id nobody was ever issued.
    match b.request(Request::DraftSnapshot { draft: 99_999 }) {
        Err(ClientError::Refused(RefusalReason::UnknownDraft)) => {}
        other => panic!("expected UnknownDraft, got {other:?}"),
    }

    // After a submit, the wire flattens the core's AlreadySubmitted into UnknownDraft, because
    // connection ownership is checked before the store is asked. A deviation from the in-process
    // contract — recorded in the report as wire-generator input.
    match a
        .request(Request::Submit { draft: a_draft })
        .expect("submit")
    {
        Response::Submitted { .. } => {}
        other => panic!("expected Submitted, got {other:?}"),
    }
    match a.request(Request::Submit { draft: a_draft }) {
        Err(ClientError::Refused(RefusalReason::UnknownDraft)) => {}
        other => panic!("expected UnknownDraft after submit, got {other:?}"),
    }
}

// =================================================================================================
// The envelope's boundary behavior on a live connection
// =================================================================================================

#[test]
fn unknown_version_and_malformed_frames_are_refused_and_the_connection_survives() {
    let path = start_daemon();
    let mut c = Client::connect(&path).expect("connect");

    let reply = c
        .send_raw(r#"{"v":999,"seq":1,"req":{"t":"ping"}}"#)
        .expect("reply");
    assert!(matches!(
        reply.frame,
        ServerFrame::Refused {
            reason: RefusalReason::UnknownVersion,
            ..
        }
    ));

    let reply = c.send_raw("this is not json").expect("reply");
    assert!(matches!(
        reply.frame,
        ServerFrame::Refused {
            reason: RefusalReason::MalformedFrame,
            ..
        }
    ));

    // The connection is still alive and typed after both refusals.
    assert!(matches!(
        c.request(Request::Ping).expect("ping"),
        Response::Pong
    ));
}

// =================================================================================================
// E — multi-client: live rebase across process boundaries; disconnect pruning
// =================================================================================================

#[test]
fn e1_live_rebase_and_conflict_across_processes() {
    let path = start_daemon();
    let mut a = Client::connect(&path).expect("connect a");
    let mut b = Client::connect(&path).expect("connect b");

    let a_draft = checkout(&mut a);
    let b_draft = checkout(&mut b);
    assert!(set_text(&mut b, b_draft, FieldName::Label, "Mine-B").is_none());
    assert!(set_text(&mut a, a_draft, FieldName::Label, "Theirs-A").is_none());
    match a
        .request(Request::Submit { draft: a_draft })
        .expect("submit")
    {
        Response::Submitted { version } => assert_eq!(version, 1),
        other => panic!("expected Submitted, got {other:?}"),
    }

    // B is told, fetches, and sees the full three-way conflict shape as values.
    assert_eq!(
        b.wait_push().expect("tick"),
        Push::CanonicalChanged { version: 1 }
    );
    assert_eq!(
        b.wait_push().expect("rebased"),
        Push::DraftRebased {
            draft: b_draft,
            base_version: 1
        }
    );
    let snap = snapshot(&mut b, b_draft);
    assert!(snap.label.dirty);
    assert_eq!(snap.label.raw, Some(RawWire::Text("Mine-B".to_string())));
    assert_eq!(
        snap.label.base,
        Some(RawWire::Text("Documents".to_string()))
    );
    assert_eq!(
        snap.label.theirs,
        Some(RawWire::Text("Theirs-A".to_string())),
        "the conflict is visible as values"
    );

    // A conflicted draft's submit is refused, typed, across the wire.
    match b
        .request(Request::Submit { draft: b_draft })
        .expect("submit")
    {
        Response::SubmitRefused {
            refusal: SubmitRefusalWire::Conflicted { fields },
        } => assert_eq!(fields, vec![FieldName::Label]),
        other => panic!("expected Conflicted, got {other:?}"),
    }

    // Resolve keep-mine (the field-level ceiling), then submit wins.
    assert!(matches!(
        b.request(Request::Resolve {
            draft: b_draft,
            field: FieldName::Label,
            keep_mine: true,
        })
        .expect("resolve"),
        Response::Resolved
    ));
    match b
        .request(Request::Submit { draft: b_draft })
        .expect("submit")
    {
        Response::Submitted { version } => assert_eq!(version, 2),
        other => panic!("expected Submitted, got {other:?}"),
    }
    match a.request(Request::CanonicalSnapshot).expect("canonical") {
        Response::Canonical { canonical: Some(c) } => assert_eq!(c.label, "Mine-B"),
        other => panic!("expected canonical, got {other:?}"),
    }
}

#[test]
fn e2_disconnect_prunes_the_connections_drafts() {
    let path = start_daemon();
    let mut b = Client::connect(&path).expect("connect b");

    let stats = |c: &mut Client| match c.request(Request::Stats).expect("stats") {
        Response::Stats { drafts, rebasing } => (drafts, rebasing),
        other => panic!("expected Stats, got {other:?}"),
    };

    {
        let mut a = Client::connect(&path).expect("connect a");
        let _a_draft = checkout(&mut a);
        assert_eq!(stats(&mut b), (1, 1));
        // `a` drops here without Close — at the socket layer an abrupt drop and a client kill -9
        // are the same FIN/EOF, which is why this stands in for the crashed-client variant.
    }

    // The daemon notices asynchronously; poll with a deadline rather than sleeping blind.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if stats(&mut b) == (0, 0) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "disconnect did not prune the dead connection's drafts (E2/C18 across the wire)"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

// =================================================================================================
// F1 (in-process variant) — a stashed draft survives into a FRESH daemon; M4 does the kill -9
// =================================================================================================

#[test]
fn f1_stash_restores_into_a_fresh_daemon_with_verdict_reset() {
    // Daemon one: edit a draft, drive its check to a PASSED verdict, stash.
    let path1 = start_daemon();
    let mut c1 = Client::connect(&path1).expect("connect 1");
    let draft1 = checkout(&mut c1);
    assert!(set_text(&mut c1, draft1, FieldName::Label, "Draft-in-progress").is_none());
    assert!(
        set_text(
            &mut c1,
            draft1,
            FieldName::Folder,
            "/Users/Shared/Elsewhere"
        )
        .is_none()
    );
    let token = match c1
        .request(Request::BeginCheck {
            draft: draft1,
            check: CheckName::FolderReachable,
        })
        .expect("begin")
    {
        Response::CheckBegun { token } => token,
        other => panic!("expected CheckBegun, got {other:?}"),
    };
    assert!(matches!(
        c1.request(Request::CompleteCheck {
            draft: draft1,
            check: CheckName::FolderReachable,
            token,
            ok: true,
        })
        .expect("complete"),
        Response::CheckSettled { accepted: true }
    ));
    assert!(
        validate(&mut c1, draft1).is_ok(),
        "checked and green before death"
    );
    let stash = match c1.request(Request::Stash { draft: draft1 }).expect("stash") {
        Response::Stashed { stash } => stash,
        other => panic!("expected Stashed, got {other:?}"),
    };

    // "The daemon dies": a second daemon with a fresh store, reached over a new socket.
    let path2 = start_daemon();
    let mut c2 = Client::connect(&path2).expect("connect 2");
    let restored = match c2.request(Request::Restore { stash }).expect("restore") {
        Response::DraftId { draft } => draft,
        other => panic!("expected DraftId, got {other:?}"),
    };

    let snap = snapshot(&mut c2, restored);
    // Dirty values survive...
    assert_eq!(
        snap.label.raw,
        Some(RawWire::Text("Draft-in-progress".to_string()))
    );
    assert!(snap.label.dirty);
    // ...the conflict-relevant base survives (same seed canonical, so no conflict here)...
    assert_eq!(
        snap.label.base,
        Some(RawWire::Text("Documents".to_string()))
    );
    assert_eq!(snap.label.theirs, None);
    // ...and the check verdict did NOT survive (C20): the dirty folder demands a fresh check.
    assert!(
        rule_keys(&snap.report).contains(&"folder_check_required"),
        "a stashed verdict must not endorse a value against a daemon that may have moved"
    );
}
