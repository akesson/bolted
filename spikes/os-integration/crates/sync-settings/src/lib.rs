//! `sync-settings` — the step-18 vehicle: the smallest feature that exercises the whole contract
//! over IPC. Macro-declared (the framework path), because the question is whether the *shipped*
//! contract crosses the wire, not whether hand-rolled code can.
//!
//! No real syncing lives here — no FSEvents, no file IO, no engine on a timer. Canonical-change
//! pressure comes from a second client submitting or toggling, which is the honest multi-process
//! story anyway (step-18 step doc, "The vehicle").
#![forbid(unsafe_code)]

pub mod value_types;

use bolted_core::{Draft, DraftId, ErrorData, StoreDraft, ValidationReport, Value};
pub use value_types::{Paused, PausedError, absolute_path, interval_in_range};

// =================================================================================================
// Tier 1 — the newtypes. `Paused` is hand-written (D20) and lives in `value_types`.
// =================================================================================================

/// Trim; 1..=30 chars.
#[bolted_macros::value]
#[sanitize(trim)]
#[validate(len_chars(min = 1, max = 30))]
pub struct SyncLabel(String);

/// Trim; 1..=120 chars; absolute (`/`-rooted).
#[bolted_macros::value]
#[sanitize(trim)]
#[validate(
    len_chars(min = 1, max = 120),
    custom(absolute_path, variant = NotAbsolute, key = "not_absolute")
)]
pub struct FolderPath(String);

/// Trim; minutes as text (what a settings box sends), whole number in `1..=1440`.
#[bolted_macros::value]
#[sanitize(trim)]
#[validate(
    len_chars(min = 1, max = 4),
    custom(interval_in_range, variant = OutOfRange, key = "interval_out_of_range")
)]
pub struct SyncInterval(String);

impl SyncInterval {
    /// The parsed minutes. A valid `SyncInterval` always parses (that is what `interval_in_range`
    /// proved), so the fallback is unreachable and exists only to keep this panic-free.
    pub fn minutes(&self) -> u32 {
        self.as_str().parse().unwrap_or(0)
    }
}

// =================================================================================================
// The entity
// =================================================================================================

/// The always-valid canonical state, and — via the macro — `SyncSettingsField`,
/// `SyncSettingsCheck`, `SyncSettingsStash`, `SyncSettingsDraft`, `SyncSettingsStore`, and the
/// four trait impls.
///
/// The `folder` check is load-bearing scope: it is how C13/C16 and the capability seam get probed
/// across IPC (the *client* drives begin/complete through the wire).
#[bolted_macros::entity(rules)]
pub struct SyncSettings {
    pub label: SyncLabel,
    #[check(
        rule = "folder_reachable",
        pending_key = "folder_check_pending",
        required_key = "folder_check_required",
        failed_key = "folder_unreachable"
    )]
    pub folder: FolderPath,
    pub interval: SyncInterval,
    pub paused: Paused,
}

// =================================================================================================
// Tier 2 — the relational rule
// =================================================================================================

#[bolted_macros::rules(entity = SyncSettings)]
impl SyncSettingsDraft {
    /// A network volume (`/Volumes/…`) may sync at most every 15 minutes.
    ///
    /// Evaluated only over valid values: an invalid or unset field is already flagged by tier 1.
    /// `pins(interval)` puts the error under the interval box, not the folder box.
    #[rule(pins(interval))]
    fn network_volume_interval(&self) -> Result<(), ErrorData> {
        if let (Some(folder), Some(interval)) = (self.folder.value(), self.interval.value())
            && folder.as_str().starts_with("/Volumes/")
            && interval.minutes() < 15
        {
            return Err(ErrorData {
                key: "network_interval_too_fast",
                params: vec![
                    ("min", "15".to_string()),
                    ("actual", interval.minutes().to_string()),
                ],
            });
        }
        Ok(())
    }
}

// =================================================================================================
// The hand-written session-less mutation (§9's demoted `command` verb — NOT designed here)
// =================================================================================================

/// Why [`toggle_paused`] refused.
#[derive(Debug, Clone, PartialEq)]
pub enum ToggleError {
    /// No canonical to toggle — the daemon has not been seeded.
    NoCanonical,
    /// The flipped entity fails full validation. Unreachable for a paused flip against an
    /// always-valid canonical, but the gate runs anyway: **submit re-validates everything, always**
    /// applies to a session-less mutation too, or the command could write a canonical no draft
    /// could ever submit.
    Validation(ValidationReport<SyncSettingsField>),
}

/// Flip `paused` on the current canonical: validate the flipped entity in full (tiers 1 + 2),
/// then [`bolted_core::Store::apply_canonical`]. Returns the new paused state and the rebase
/// fan-out, exactly as the store reports it.
///
/// Hand-written on purpose. §9 demoted the `command` verb pending "a real feature that needs a
/// session-less mutation" — this is plausibly its first real customer, and the evidence goes in
/// the step-18 report, not into a verb design. Two observations already bank themselves in the
/// shape of this function: tier-1 validity is free (canonical is always-valid and the flip happens
/// inside a value type), and tier-2 rules are NOT free — `apply_canonical` runs no rules, so a
/// command that skipped the scratch-draft validation below would bypass them silently.
pub fn toggle_paused(
    store: &mut SyncSettingsStore,
) -> Result<(bool, Vec<DraftId>), Box<ToggleError>> {
    let Some(current) = store.canonical() else {
        return Err(Box::new(ToggleError::NoCanonical));
    };
    let flipped = !current.paused.is_on();
    let mut next = current.clone();
    next.paused = match Paused::try_new(flipped) {
        Ok(p) => p,
        Err(e) => match e {},
    };
    // Full re-validation through a scratch checkout of the flipped entity. Tier 1 cannot fail
    // (every field is a Value); tier 2 can, in principle, so it gates.
    let scratch = SyncSettingsDraft::from_canonical(Some(&next), 0);
    let report = scratch.validate();
    if !report.is_ok() {
        return Err(Box::new(ToggleError::Validation(report)));
    }
    Ok((flipped, store.apply_canonical(next)))
}

/// The canonical the daemon seeds at boot (persistence is a VISION optional battery, out of
/// step-18 scope — the gap is recorded in the report). `None` only if the seed literals stopped
/// validating, which the unit test pins.
pub fn seed() -> Option<SyncSettings> {
    Some(SyncSettings {
        label: SyncLabel::try_new("Documents".to_string()).ok()?,
        folder: FolderPath::try_new("/Users/Shared/Documents".to_string()).ok()?,
        interval: SyncInterval::try_new("30".to_string()).ok()?,
        paused: Paused::try_new(false).ok()?,
    })
}

/// The store is `Send` by construction (D16) — proved at rung 1, as every feature crate does.
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<SyncSettingsStore>();
    assert_send::<SyncSettingsDraft>();
    assert_send::<SyncSettings>();
};
