//! Host-side controller tests — the bulk of step-04's automated coverage (no browser, no Leptos).
//! The analog of step-03's headless `ProfileViewModel` tests, one tier down: these run against the
//! **real `bolted_core::Store`** consumed as a plain crate, which is the first time that path has a
//! UI-shaped consumer at all (the FFI wrapper re-owned the loop and bypassed `Store`).
//!
//! The four behaviours on trial (echo rule / live rebase / conflict resolution / submit) plus the
//! async check and F3-on-the-real-store. A test may name a constraint value; shell code may not.

use bolted_core::{CheckState, Draft, DraftStatus, ErrorData, SyncState, Validity};
use profile_web::controller::{
    ProfileController, SubmitOutcome, is_required, max_len, simulated_lookup,
};
use spike_profile::ProfileDraft;
use spike_profile::ProfileField::{Availability, Email, Name, Username};
use std::cell::Ref;

fn controller() -> ProfileController {
    ProfileController::new().expect("the seed profile validates")
}

/// `ProfileController::draft()` yields `None` once the handle is a tombstone (C17). This shell
/// never observes that state — a successful submit checks out a fresh draft in the same call — so
/// the tests read through here rather than threading an `Option` into every assertion.
fn draft(c: &ProfileController) -> Ref<'_, ProfileDraft> {
    c.draft().expect("this shell always holds a live draft")
}

/// Drive the uniqueness check to a pass. C16 refuses a submit whose dirty username was never
/// checked, so any test that edits the username and expects to submit must do this — as must any
/// real shell.
fn pass_check(c: &mut ProfileController, ticket: u64) {
    assert_eq!(run_check(c, ticket), Some(true), "the check must land");
}

/// Drive a debounced check to completion the way the view layer does: the timer for `ticket`
/// fires, the core hands back a token, the (shell-side) lookup runs, the verdict comes back.
fn run_check(c: &mut ProfileController, ticket: u64) -> Option<bool> {
    let (token, name) = c.fire_check_if_current(ticket)?;
    let verdict = simulated_lookup(&name);
    Some(c.complete_check(token, verdict))
}

// ---- constraint-derived affordances (no literal in shell code) ---------------------------------

#[test]
fn affordances_derive_from_core_constraints() {
    assert_eq!(max_len(Username), Some(20));
    assert_eq!(max_len(Name), Some(30));
    assert_eq!(max_len(Email), None); // Email declares no LenChars — the counter must vanish
    assert!(is_required(Username) && is_required(Availability));
}

// ---- echo rule (§6) ----------------------------------------------------------------------------

#[test]
fn echo_rule_focused_buffer_is_never_rewritten_from_core() {
    let mut c = controller();
    c.focus(Username);
    c.edit_username("  bob_1  ".to_string());

    // The core sanitized (trim) and validated per keystroke...
    assert_eq!(
        draft(&c).username.value().map(|u| u.as_str()),
        Some("bob_1")
    );
    // ...but the focused buffer still holds exactly what the user typed. Cursor safety.
    assert_eq!(c.username_buf(), "  bob_1  ");

    // An external event that refreshes buffers must still not touch the focused field.
    c.sim_set_name("Server Name");
    assert_eq!(c.username_buf(), "  bob_1  ");
    assert_eq!(c.name_buf(), "Server Name"); // ...while an unfocused clean field adopts

    // Blur hands ownership back to the core: the buffer refreshes to the sanitized value.
    c.blur(Username);
    assert_eq!(c.username_buf(), "bob_1");
}

#[test]
fn echo_rule_invalid_raw_is_preserved_through_blur() {
    let mut c = controller();
    c.focus(Username);
    c.edit_username("ab".to_string()); // too short

    assert!(matches!(
        draft(&c).username.validity(),
        Validity::Invalid { .. }
    ));
    assert_eq!(
        c.inline_error(Username).as_deref(),
        Some("Too short — minimum 3, got 2.")
    );

    // The rejected text survives the blur — the core retained it as `Invalid.raw`.
    c.blur(Username);
    assert_eq!(c.username_buf(), "ab");
}

#[test]
fn echo_rule_email_lowercasing_defers_to_blur() {
    let mut c = controller();
    c.focus(Email);
    c.edit_email("Foo@BAR.com".to_string());
    assert_eq!(
        draft(&c).email.value().map(|e| e.as_str()),
        Some("foo@bar.com")
    );
    assert_eq!(c.email_buf(), "Foo@BAR.com");
    c.blur(Email);
    assert_eq!(c.email_buf(), "foo@bar.com");
}

#[test]
fn reverting_to_the_base_value_clears_dirty() {
    let mut c = controller();
    c.edit_name("Bob".to_string());
    assert!(c.is_dirty(Name) && c.any_dirty());
    c.edit_name("Alice Smith".to_string());
    assert!(!c.is_dirty(Name) && !c.any_dirty()); // revert-for-free (C5)
}

// ---- the composite value object (grouped setter) ------------------------------------------------

#[test]
fn reversed_date_range_is_invalid_and_renders_the_core_sentence() {
    let mut c = controller();
    c.edit_end("2025-01-01".to_string()); // before the seed's start
    assert!(matches!(
        draft(&c).availability.validity(),
        Validity::Invalid { .. }
    ));
    assert_eq!(
        c.inline_error(Availability).as_deref(),
        Some("Start must be on or before end.")
    );
}

// ---- live rebase (§4) ---------------------------------------------------------------------------

#[test]
fn live_rebase_clean_field_adopts_silently() {
    let mut c = controller();
    c.sim_set_name("Server Name");

    assert!(matches!(draft(&c).name.sync(), SyncState::InSync));
    assert_eq!(
        draft(&c).name.value().map(|n| n.as_str()),
        Some("Server Name")
    );
    assert!(!c.is_dirty(Name));
    assert_eq!(c.name_buf(), "Server Name");
    assert!(c.conflict(Name).is_none());
}

#[test]
fn live_rebase_dirty_field_conflicts_and_preserves_mine() {
    let mut c = controller();
    c.edit_name("My Name".to_string());
    c.sim_set_name("Server Name");

    // Yours is preserved in the validity dimension; the sync dimension carries the 3-way data.
    assert_eq!(draft(&c).name.value().map(|n| n.as_str()), Some("My Name"));
    assert_eq!(c.name_buf(), "My Name");
    let info = c.conflict(Name).expect("conflicted");
    assert_eq!(info.theirs, "Server Name");
    assert_eq!(info.base.as_deref(), Some("Alice Smith"));
    assert_eq!(c.conflicts(), vec![Name]);
}

/// C19, where a user would actually meet it: I am editing one field, the server changes a
/// *different* one. The store rebases the whole draft, so my field is rebased onto its own
/// ancestor — and until step 07 that raised a conflict banner offering a "take theirs" button
/// holding my own base value, and refused submit.
///
/// This tier should have caught it: `echo_rule_focused_buffer_is_never_rewritten_from_core` has
/// been dirtying `username` and then calling `sim_set_name` since step 04. It only ever asserted
/// on the buffers.
#[test]
fn live_rebase_leaves_a_dirty_field_alone_when_its_own_canonical_did_not_move() {
    let mut c = controller();
    c.edit_name("My Name".to_string());
    c.sim_set_email("team@corp.example"); // the server touches email, and only email

    assert_eq!(c.conflicts(), vec![], "`name`'s canonical never moved");
    assert!(c.conflict(Name).is_none());
    assert!(c.is_dirty(Name));
    assert_eq!(c.name_buf(), "My Name");
    assert!(matches!(draft(&c).name.sync(), SyncState::InSync));

    // ...and the draft still submits, because there is nothing to resolve.
    assert_eq!(c.conflicts(), vec![]);
}

#[test]
fn live_rebase_convergent_edit_lands_clean() {
    let mut c = controller();
    c.edit_name("Server Name".to_string()); // the same edit the "server" is about to make
    c.sim_set_name("Server Name");
    assert!(matches!(draft(&c).name.sync(), SyncState::InSync));
    assert!(!c.is_dirty(Name)); // C04
}

/// **D9, the sharpened echo rule.** The control owns its text while focused *and dirty*. A focused
/// field the user never typed into holds nothing worth protecting, so a rebase repaints it at once.
///
/// Before the freeze this field stayed stale until blur, and the running app showed the canonical
/// pane and the focused field disagreeing with nothing on screen to explain it (step-04). This is
/// the §9 case XCUITest could not drive (focus/blur cannot be ordered against an async rebase),
/// pinned here exactly as the Swift shell pins it.
#[test]
fn live_rebase_focused_clean_field_adopts_live() {
    let mut c = controller();
    c.focus(Name);
    c.sim_set_name("Server Name");

    assert_eq!(
        draft(&c).name.value().map(|n| n.as_str()),
        Some("Server Name")
    ); // core: current
    assert_eq!(c.name_buf(), "Server Name"); // screen: current too
    assert!(!c.is_dirty(Name));
}

/// ...and the protection the echo rule *does* give: a focused **dirty** field is never repainted
/// from the core, so per-keystroke sanitization can never move the caret.
#[test]
fn live_rebase_focused_dirty_field_keeps_the_users_text() {
    let mut c = controller();
    c.focus(Name);
    c.edit_name("My Name".to_string());
    c.sim_set_name("Server Name");

    assert_eq!(c.name_buf(), "My Name");
    assert_eq!(c.conflict(Name).expect("conflicted").theirs, "Server Name");
}

#[test]
fn canonical_deletion_orphans_the_draft() {
    let mut c = controller();
    c.sim_delete();
    assert!(!c.is_live());
    assert!(matches!(draft(&c).status(), DraftStatus::Orphaned));
    assert!(c.canonical_view().is_none());
}

// ---- conflict resolution (the framework ceiling) ------------------------------------------------

#[test]
fn resolve_keep_mine_rebases_the_base_and_stays_dirty() {
    let mut c = controller();
    c.edit_name("My Name".to_string());
    c.sim_set_name("Server Name");
    c.resolve_keep_mine(Name);

    assert_eq!(draft(&c).name.value().map(|n| n.as_str()), Some("My Name"));
    assert_eq!(
        draft(&c).name.base().map(|n| n.as_str()),
        Some("Server Name")
    );
    assert!(matches!(draft(&c).name.sync(), SyncState::InSync));
    assert!(c.is_dirty(Name)); // C09
    assert_eq!(c.name_buf(), "My Name");
}

#[test]
fn resolve_take_theirs_adopts_and_cleans_even_when_focused() {
    let mut c = controller();
    c.focus(Name);
    c.edit_name("My Name".to_string());
    c.sim_set_name("Server Name");
    c.resolve_take_theirs(Name);

    assert_eq!(
        draft(&c).name.value().map(|n| n.as_str()),
        Some("Server Name")
    );
    assert!(!c.is_dirty(Name));
    assert!(matches!(draft(&c).name.sync(), SyncState::InSync));
    // The value moved from *outside* a keystroke, so the focused buffer IS refreshed (the echo
    // rule's one exception — the control no longer owns a value the user did not type).
    assert_eq!(c.name_buf(), "Server Name");
}

/// **C14 (was F6).** Typing a conflicted field until it equals *theirs* now resolves the conflict,
/// exactly as a convergent rebase does when the canonical change arrives second (C04).
///
/// The old behaviour left a "Keep mine / Take theirs" banner on screen whose two buttons did
/// visibly the same thing, beside a lit dirty marker. The running app's verdict was that a user
/// cannot tell what is being asked.
#[test]
fn c14_conflicted_field_edited_to_equal_theirs_auto_converges() {
    let mut c = controller();
    c.edit_name("My Name".to_string());
    c.sim_set_name("Server Name");
    c.edit_name("Server Name".to_string());

    assert!(matches!(draft(&c).name.sync(), SyncState::InSync));
    assert!(c.conflicts().is_empty());
    assert!(!c.is_dirty(Name));
    assert!(c.conflict(Name).is_none()); // and the banner is gone
}

/// C13, visible through the shell: take-theirs on username moves its value, so a completed
/// uniqueness verdict cannot survive it.
#[test]
fn take_theirs_on_username_resets_the_async_check() {
    let mut c = controller();
    let ticket = c.edit_username("bob_1".to_string());
    assert_eq!(run_check(&mut c, ticket), Some(true));
    assert!(matches!(
        c.username_check(),
        CheckState::Done { verdict: Ok(()) }
    ));

    // A conflicting rebase preserves *my* value, so the verdict still endorses what it checked.
    c.sim_set_username("server_user");
    assert!(matches!(
        c.username_check(),
        CheckState::Done { verdict: Ok(()) }
    ));

    // Taking theirs changes the value → the verdict is un-endorsed.
    c.resolve_take_theirs(Username);
    assert!(matches!(c.username_check(), CheckState::Idle));
}

#[test]
fn rebase_adopting_username_resets_the_async_check() {
    let mut c = controller();
    let ticket = c.edit_username("bob_1".to_string());
    assert_eq!(run_check(&mut c, ticket), Some(true));

    // Revert to base → clean → the next rebase adopts theirs → value moves → check resets (C13).
    c.edit_username("alice".to_string());
    assert!(matches!(c.username_check(), CheckState::Idle)); // the edit itself already reset it
    c.sim_set_username("server_user");
    assert_eq!(
        draft(&c).username.value().map(|u| u.as_str()),
        Some("server_user")
    );
    assert!(matches!(c.username_check(), CheckState::Idle));
}

// ---- the async uniqueness check (single-flight, driven shell-side) -------------------------------

#[test]
fn debounce_collapses_a_burst_into_a_single_check() {
    let mut c = controller();
    let tickets: Vec<u64> = ["b", "bo", "bob", "bob_", "bob_1"]
        .iter()
        .map(|s| c.edit_username((*s).to_string()))
        .collect();

    // Every superseded timer fires into a no-op; only the last ticket begins a check.
    for t in &tickets[..tickets.len() - 1] {
        assert!(c.fire_check_if_current(*t).is_none());
    }
    assert_eq!(run_check(&mut c, tickets[4]), Some(true));
    assert_eq!(c.check_run_count(), 1);
}

#[test]
fn a_clean_username_is_never_checked() {
    let mut c = controller();
    let ticket = c.edit_username("alice".to_string()); // == base: valid but not dirty
    assert!(c.fire_check_if_current(ticket).is_none());
    assert_eq!(c.check_run_count(), 0);
}

#[test]
fn an_invalid_username_is_never_checked() {
    let mut c = controller();
    let ticket = c.edit_username("ab".to_string());
    assert!(c.fire_check_if_current(ticket).is_none());
    assert_eq!(c.check_run_count(), 0);
}

#[test]
fn check_states_are_observable_idle_pending_done() {
    let mut c = controller();
    assert!(matches!(c.username_check(), CheckState::Idle));

    let ticket = c.edit_username("bob_1".to_string());
    let (token, name) = c.fire_check_if_current(ticket).expect("valid + dirty");
    assert!(c.is_checking()); // Pending — the spinner binds to exactly this
    assert!(matches!(c.username_check(), CheckState::Pending { .. }));

    assert!(c.complete_check(token, simulated_lookup(&name)));
    assert!(!c.is_checking());
    assert!(matches!(
        c.username_check(),
        CheckState::Done { verdict: Ok(()) }
    ));
}

/// Typing through a pending check invalidates the in-flight verdict: the value changed, so the
/// core reset the check (C13) and the late completion is discarded by sequence (C10). No shell
/// bookkeeping — the spinner behaviour falls out of the contract.
#[test]
fn a_value_change_during_pending_discards_the_late_verdict() {
    let mut c = controller();
    let ticket = c.edit_username("admin".to_string());
    let (token, name) = c.fire_check_if_current(ticket).expect("valid + dirty");
    assert!(c.is_checking());

    c.edit_username("admin2".to_string()); // types on, mid-flight
    assert!(matches!(c.username_check(), CheckState::Idle));

    assert!(!c.complete_check(token, simulated_lookup(&name))); // stale → ignored
    assert!(matches!(c.username_check(), CheckState::Idle));
    assert!(c.inline_error(Username).is_none()); // never endorses (or condemns) the wrong text
}

#[test]
fn a_taken_username_surfaces_the_core_verdict_inline() {
    let mut c = controller();
    let ticket = c.edit_username("admin".to_string());
    assert_eq!(run_check(&mut c, ticket), Some(true));

    assert!(matches!(
        c.username_check(),
        CheckState::Done { verdict: Err(_) }
    ));
    assert_eq!(
        c.inline_error(Username).as_deref(),
        Some("That username is already taken.")
    );
}

// ---- submit (tier 3) -----------------------------------------------------------------------------

#[test]
fn submit_invalid_returns_a_validation_report() {
    let mut c = controller();
    c.edit_name(String::new()); // empty after trim → Invalid
    c.submit();

    let Some(SubmitOutcome::Validation(report)) = c.last_submit() else {
        panic!("expected a validation report, got {:?}", c.last_submit());
    };
    assert_eq!(report.field_errors.len(), 1);
    assert_eq!(report.field_errors[0].0, Name);
    assert!(c.is_live()); // the draft survives
}

#[test]
fn submit_surfaces_the_tier2_rule_error() {
    let mut c = controller();
    let ticket = c.edit_username("corp_bob".to_string());
    pass_check(&mut c, ticket); // otherwise C16 refuses first, and the tier-2 rule never shows
    c.submit(); // email is still alice@example.com, not the corp domain

    let Some(SubmitOutcome::Validation(report)) = c.last_submit() else {
        panic!("expected a validation report, got {:?}", c.last_submit());
    };
    assert_eq!(report.rule_errors.len(), 1);
    assert_eq!(report.rule_errors[0].rule, "corporate_email");
    assert_eq!(report.rule_errors[0].pins, vec![Email]); // the rule pins its error to Email
}

/// **F3 on the real `bolted_core::Store::submit`** — the first time this path runs against the
/// store rather than the FFI wrapper's re-owned loop. A refused submit hands the draft back: the
/// edit session survives, the conflict resolves, the resubmit succeeds.
#[test]
fn submit_conflicted_is_refused_and_leaves_the_draft_alive() {
    let mut c = controller();
    c.edit_name("My Name".to_string());
    c.sim_set_name("Server Name");
    c.submit();

    assert!(matches!(c.last_submit(), Some(SubmitOutcome::Conflicted(f)) if f == &[Name]));
    assert!(c.is_live());
    // The draft is not just alive but *editable*, with my value intact.
    assert_eq!(draft(&c).name.value().map(|n| n.as_str()), Some("My Name"));

    c.resolve_keep_mine(Name);
    c.submit();
    assert!(matches!(c.last_submit(), Some(SubmitOutcome::Success)));
    assert_eq!(c.canonical_view().expect("canonical").name, "My Name");
}

#[test]
fn submit_success_updates_canonical_and_rechecks_out() {
    let mut c = controller();
    c.edit_name("My Name".to_string());
    c.edit_email("bob@example.com".to_string());
    c.submit();

    assert!(matches!(c.last_submit(), Some(SubmitOutcome::Success)));

    // Final truth arrives via `store.canonical()`, never the shell's own input echoed back.
    let canonical = c.canonical_view().expect("canonical");
    assert_eq!(canonical.name, "My Name");
    assert_eq!(canonical.email, "bob@example.com");

    // The shell re-checked-out: a fresh, clean draft based on the committed entity...
    assert!(c.is_live() && !c.any_dirty());
    assert_eq!(c.name_buf(), "My Name");
    // ...and it is registered for live rebase, like any other checkout.
    c.sim_set_name("Server Name");
    assert_eq!(
        draft(&c).name.value().map(|n| n.as_str()),
        Some("Server Name")
    );
    assert!(!c.is_dirty(Name));
}

#[test]
fn submit_on_an_orphaned_draft_is_a_typed_refusal() {
    let mut c = controller();
    c.edit_name("My Name".to_string());
    c.sim_delete();
    c.submit();
    assert!(matches!(c.last_submit(), Some(SubmitOutcome::Orphaned)));
}

/// **C16 (was F2).** A username that was never checked can no longer reach a passing submit.
///
/// This is the finding that made the freeze act: `"admin"` is taken, the check would have refused
/// it, and before the freeze `CheckState::Idle` sailed through `validate()` — on *both* shells, on
/// the *default* path (any submit that beats the 400 ms debounce). Now the core refuses, typed, and
/// the shell's own debounced check unblocks it.
#[test]
fn c16_an_unrun_check_on_a_dirty_username_blocks_submit() {
    let mut c = controller();
    let ticket = c.edit_username("admin".to_string());
    assert!(matches!(c.username_check(), CheckState::Idle));
    c.submit();

    let Some(SubmitOutcome::Validation(report)) = c.last_submit() else {
        panic!("expected a refusal, got {:?}", c.last_submit());
    };
    assert_eq!(
        report.rule_errors[0].error,
        ErrorData::new("username_check_required")
    );
    assert_eq!(report.rule_errors[0].pins, vec![Username]);
    assert_eq!(c.canonical_view().expect("canonical").username, "alice");

    // Let the check run: "admin" is taken, so the submit is still refused — now on the merits.
    assert_eq!(run_check(&mut c, ticket), Some(true));
    c.submit();
    let Some(SubmitOutcome::Validation(report)) = c.last_submit() else {
        panic!("expected a refusal, got {:?}", c.last_submit());
    };
    assert_eq!(
        report.rule_errors[0].error,
        ErrorData::new("username_taken")
    );
}

/// ...and a clean username never needs a check to submit: it still holds the canonical value, which
/// was verified when it was committed. Without this half, editing only your email would be
/// unsubmittable until a pointless uniqueness lookup ran.
#[test]
fn c16_a_clean_username_needs_no_check_to_submit() {
    let mut c = controller();
    c.edit_email("bob@example.com".to_string());
    assert!(matches!(c.username_check(), CheckState::Idle));
    assert!(!c.is_dirty(Username));
    c.submit();

    assert!(matches!(c.last_submit(), Some(SubmitOutcome::Success)));
    assert_eq!(c.check_run_count(), 0);
}

/// C17 through the shell: a successful submit tombstones the handle, and this controller
/// immediately checks out a fresh draft — so the tombstone is never observable from the outside.
#[test]
fn c17_submit_tombstones_the_handle_and_the_shell_rechecks_out() {
    let mut c = controller();
    c.edit_name("My Name".to_string());
    c.submit();

    assert!(matches!(c.last_submit(), Some(SubmitOutcome::Success)));
    assert!(c.draft().is_some(), "the shell holds a fresh draft");
    assert!(c.is_live() && !c.any_dirty());
}

/// A pending check *does* block submit (it is modelled as a rule violation pinned to Username),
/// so the shell never commits while a verdict is in flight.
#[test]
fn a_pending_check_blocks_submit() {
    let mut c = controller();
    let ticket = c.edit_username("bob_1".to_string());
    let _ = c.fire_check_if_current(ticket).expect("valid + dirty");
    c.submit();

    let Some(SubmitOutcome::Validation(report)) = c.last_submit() else {
        panic!("expected a validation report, got {:?}", c.last_submit());
    };
    assert_eq!(
        report.rule_errors[0].error,
        ErrorData::new("username_check_pending")
    );
}

/// The reason D9's predicate is **touched**, not `dirty`.
///
/// The user types trailing spaces over the base value. The core trims them, so the value never
/// moved and the field is *clean* — while the buffer holds live keystrokes. Had `refresh_buffers`
/// keyed on `is_dirty()`, an unrelated server change would repaint this field, eat the spaces and
/// jump the caret. `dirty` and `touched` agree everywhere except here, and here `dirty` is wrong.
#[test]
fn echo_rule_a_focused_field_that_sanitizes_back_to_base_still_keeps_its_text() {
    let mut c = controller();
    c.focus(Username);
    c.edit_username("  alice  ".to_string()); // trims to "alice" == base -> valid, NOT dirty
    assert!(!c.is_dirty(Username));

    c.sim_set_name("Server Name"); // an unrelated field changed on the server
    assert_eq!(c.username_buf(), "  alice  ", "the caret must not move");

    // ...and blur still hands ownership back to the core.
    c.blur(Username);
    assert_eq!(c.username_buf(), "alice");
}
