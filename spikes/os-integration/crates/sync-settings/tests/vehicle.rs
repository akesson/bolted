//! M1 — the vehicle in-process, before any wire exists: checkout / edit / submit / check /
//! toggle. These are baseline assertions the M2 integration tests repeat over a real socket;
//! a divergence between the two is then attributable to the wire, not the feature.

use bolted_core::{CheckState, Checked, Draft, ErrorData, Stashable, Store, SubmitError, Value};
use sync_settings::{
    SyncSettings, SyncSettingsCheck, SyncSettingsDraft, SyncSettingsField, seed, toggle_paused,
};

fn seeded() -> Store<SyncSettingsDraft> {
    Store::new(Some(seed().expect("the seed literals validate")))
}

#[test]
fn the_seed_validates() {
    assert!(seed().is_some());
}

#[test]
fn checkout_edit_submit_bumps_canonical() {
    let mut store = seeded();
    let id = store.checkout();
    let v0 = store.version();

    let draft = store.draft_mut(id).expect("live");
    draft.try_set_label("Photos".to_string()).expect("valid");
    // The folder is clean (not dirty), so C16 does not demand a check run and submit may pass.
    store.submit(id).expect("submits");

    assert_eq!(store.version(), v0 + 1);
    let canonical = store.canonical().expect("seeded");
    assert_eq!(canonical.label.as_str(), "Photos");
    assert!(!store.is_live(id), "submit released the draft (C17)");
}

#[test]
fn tier1_rejection_is_keyed_data_and_blocks_submit() {
    let mut store = seeded();
    let id = store.checkout();

    let draft = store.draft_mut(id).expect("live");
    let err = draft
        .try_set_folder("relative/path".to_string())
        .expect_err("not absolute");
    assert_eq!(ErrorData::from(err).key, "not_absolute");

    let err = draft
        .try_set_interval("9999".to_string())
        .expect_err("out of range");
    assert_eq!(ErrorData::from(err).key, "interval_out_of_range");

    match store.submit(id) {
        Err(SubmitError::Validation(report)) => {
            let keys: Vec<&str> = report.field_errors.iter().map(|(_, e)| e.key).collect();
            assert!(keys.contains(&"not_absolute"));
            assert!(keys.contains(&"interval_out_of_range"));
        }
        other => panic!("expected a validation refusal, got {other:?}"),
    }
    assert!(store.is_live(id), "a refused submit keeps the draft (F3)");
}

#[test]
fn tier2_rule_pins_to_interval() {
    let mut store = seeded();
    let id = store.checkout();

    let draft = store.draft_mut(id).expect("live");
    draft
        .try_set_folder("/Volumes/NAS/Photos".to_string())
        .expect("valid folder");
    draft.try_set_interval("5".to_string()).expect("valid text");

    // The folder is now dirty and carries a check — settle it so the rule is the only violation.
    let token = draft.begin_check(SyncSettingsCheck::FolderReachable);
    assert!(draft.complete_check(SyncSettingsCheck::FolderReachable, token, Ok(())));

    let report = draft.validate();
    assert!(report.field_errors.is_empty(), "both fields are valid");
    let rule = report
        .rule_errors
        .iter()
        .find(|v| v.rule == "network_volume_interval")
        .expect("the rule fires");
    assert_eq!(rule.pins, vec![SyncSettingsField::Interval]);
    assert_eq!(rule.error.key, "network_interval_too_fast");
}

#[test]
fn async_check_gates_a_dirty_folder_and_is_value_bound() {
    let mut store = seeded();
    let id = store.checkout();
    let draft = store.draft_mut(id).expect("live");
    draft
        .try_set_folder("/Users/Shared/Other".to_string())
        .expect("valid");

    // C16: dirty + unrun check blocks with the required key.
    let report = draft.validate();
    assert_eq!(report.rule_errors.len(), 1);
    assert_eq!(report.rule_errors[0].error.key, "folder_check_required");

    // Begin → pending; a stale token's completion is discarded (C10).
    let stale = draft.begin_check(SyncSettingsCheck::FolderReachable);
    let fresh = draft.begin_check(SyncSettingsCheck::FolderReachable);
    assert!(!draft.complete_check(SyncSettingsCheck::FolderReachable, stale, Ok(())));
    assert!(draft.complete_check(SyncSettingsCheck::FolderReachable, fresh, Ok(())));
    assert!(draft.validate().is_ok());

    // C13: moving the folder's value resets the verdict.
    draft
        .try_set_folder("/Users/Shared/Third".to_string())
        .expect("valid");
    assert_eq!(
        draft.check_state(SyncSettingsCheck::FolderReachable),
        &CheckState::Idle
    );
}

#[test]
fn toggle_paused_flips_reports_fanout_and_revalidates() {
    let mut store = seeded();
    assert!(!store.canonical().expect("seeded").paused.is_on());

    // A clean checkout registers for rebase, so the toggle's fan-out must name it.
    let id = store.checkout();

    let (paused, rebased) = toggle_paused(&mut store).expect("toggles");
    assert!(paused);
    assert_eq!(rebased, vec![id], "the fan-out is the store's, as data");
    assert!(store.canonical().expect("seeded").paused.is_on());

    let (paused, _) = toggle_paused(&mut store).expect("toggles back");
    assert!(!paused);
}

#[test]
fn toggle_paused_without_canonical_is_a_typed_refusal() {
    let mut store: Store<SyncSettingsDraft> = Store::new(None);
    let err = toggle_paused(&mut store).expect_err("nothing to toggle");
    assert!(matches!(*err, sync_settings::ToggleError::NoCanonical));
}

#[test]
fn stash_roundtrips_a_dirty_draft() {
    let mut store = seeded();
    let id = store.checkout();
    let draft = store.draft_mut(id).expect("live");
    draft.try_set_label("Music".to_string()).expect("valid");

    let stash = draft.stash();
    let restored = store.restore(&stash);
    let back = store.draft(restored).expect("live");
    assert_eq!(back.dirty_fields(), vec![SyncSettingsField::Label]);
}

#[test]
fn paused_crosses_the_draft_as_a_bool_raw() {
    // The one non-String raw in the feature: the macro path must carry it end to end.
    let mut store = seeded();
    let id = store.checkout();
    let draft = store.draft_mut(id).expect("live");
    draft.try_set_paused(true).expect("infallible");
    assert_eq!(draft.dirty_fields(), vec![SyncSettingsField::Paused]);
    store.submit(id).expect("submits");
    assert!(store.canonical().expect("seeded").paused.is_on());
    let _ = SyncSettings {
        paused: Value::try_new(true).expect("infallible"),
        ..store.canonical().expect("seeded").clone()
    };
}
