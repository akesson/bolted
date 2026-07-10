//! The framework-light controller over `ProfileStore` + `ProfileHandle` — the analog of
//! step-03's `ProfileViewModel`, which ran headless. The four behaviours are tested at this
//! level with plain `cargo test` (no browser, no Leptos); the view layer above adds only *when*
//! (events, debounce timers, the version tick), never *what*.
//!
//! Unlike the Swift shell, there is no snapshot stream and no DTO layer here: every read goes
//! straight through `handle.borrow()` into the live core draft, and every affordance (counters,
//! required markers, error sentences) derives from `ProfileField::constraints()` + `ErrorData`.
//! No constraint literal appears in this crate.

use crate::l10n;
use bolted_core::{
    CheckState, CheckToken, Constraint, Draft, DraftStatus, ErrorData, Field, SubmitError,
    ValidationReport, Validity, Value,
};
use spike_profile::{
    Date, DateRange, Email, PersonName, Profile, ProfileDraft, ProfileField, ProfileHandle,
    ProfileStore, Username,
};
use std::cell::Ref;

/// The demo profile the store is seeded with — same values as the Swift app (`ProfileApp.swift`),
/// so the two shells run the identical manual protocol. Simulator *data*, not constraints.
/// `None` only if a seed literal stops satisfying its own value type — a programming error
/// surfaced in the UI (the app renders a failure note), never a panic (library-code rule).
pub fn seed_profile() -> Option<Profile> {
    Some(Profile {
        username: Username::try_new("alice".to_string()).ok()?,
        name: PersonName::try_new("Alice Smith".to_string()).ok()?,
        email: Email::try_new("alice@example.com".to_string()).ok()?,
        availability: DateRange::try_new((Date::new(2026, 1, 1), Date::new(2026, 12, 31))).ok()?,
    })
}

/// The simulated "server side" of the async uniqueness check: an in-memory taken-set, so the
/// manual tester can see a failed verdict without a backend. Same set as the Swift shell's
/// `DefaultChecker`. The *shell* owns this — the core only ever sees the `Result` verdict.
pub fn simulated_lookup(username: &str) -> Result<(), ErrorData> {
    const TAKEN: [&str; 3] = ["taken", "admin", "root"];
    if TAKEN.contains(&username.to_lowercase().as_str()) {
        Err(ErrorData::new("username_taken"))
    } else {
        Ok(())
    }
}

/// The outcome of the most recent submit, for rendering the report / success note.
#[derive(Debug, Clone)]
pub enum SubmitOutcome {
    Success,
    Validation(ValidationReport<ProfileField>),
    Conflicted(Vec<ProfileField>),
    Orphaned,
    /// The handle was already a tombstone. Unreachable in this shell (a successful submit
    /// immediately checks out a fresh draft), but the contract has the variant, so the shell does.
    AlreadySubmitted,
}

/// Conflict banner data: the incoming `theirs` (and the common-ancestor `base`) as display text.
/// *Yours* is the field's own validity, already on screen.
#[derive(Debug, Clone, PartialEq)]
pub struct ConflictInfo {
    pub base: Option<String>,
    pub theirs: String,
}

/// The canonical (server) entity as display text, for the simulator pane. Read from
/// `store.canonical()` — never from the shell's own inputs echoed back.
#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalView {
    pub username: String,
    pub name: String,
    pub email: String,
    pub availability: String,
}

/// Constraint-derived UI affordances (ARCHITECTURE §1: no constraint literal in shell code).
pub fn max_len(field: ProfileField) -> Option<u32> {
    field.constraints().iter().find_map(|c| match c {
        Constraint::LenChars { max, .. } => Some(*max),
        _ => None,
    })
}

pub fn is_required(field: ProfileField) -> bool {
    field.constraints().contains(&Constraint::Required)
}

/// The controller: one long-lived store + one live draft handle + the per-field editing buffers
/// (the echo rule, §6). The buffers hold exactly what the user typed; the core's sanitized value
/// is written into them only on blur or an external move (rebase adopt / resolution / submit).
pub struct ProfileController {
    store: ProfileStore,
    handle: ProfileHandle,
    seed: Profile,
    focused: Option<ProfileField>,
    /// Has the user typed into the focused field since the core last wrote its buffer?
    ///
    /// This is what §6's echo rule actually protects. `is_dirty()` is *not* a substitute: a user
    /// typing `"  alice  "` over the base value `"alice"` produces a **clean** field (the core trims,
    /// so the value never moved) whose buffer nonetheless holds live keystrokes. Repainting it would
    /// eat the spaces and move the caret. Shell-local presentation state about a text control —
    /// not the core-side `touched` flag ARCHITECTURE §8 rejected.
    focused_touched: bool,
    username_buf: String,
    name_buf: String,
    email_buf: String,
    /// Date buffers hold the `<input type="date">` wire format (`YYYY-MM-DD`) — a widget raw
    /// form the shell converts, not a constraint (start≤end lives in `DateRange::try_new`).
    start_buf: String,
    end_buf: String,
    last_submit: Option<SubmitOutcome>,
    /// Debounce ticket generation: each username edit invalidates all earlier tickets, so a
    /// burst of keystrokes collapses to (at most) the last ticket's check — deterministic and
    /// host-testable, no timer in the controller. The *delay* is the view layer's (shell taste).
    edit_gen: u64,
    check_run_count: u32,
}

impl ProfileController {
    /// Seed the store, check out the draft, fill the buffers. `None` iff the seed fails.
    pub fn new() -> Option<Self> {
        let seed = seed_profile()?;
        let mut store = ProfileStore::new(Some(seed.clone()));
        let handle = store.checkout();
        let mut c = ProfileController {
            store,
            handle,
            seed,
            focused: None,
            focused_touched: false,
            username_buf: String::new(),
            name_buf: String::new(),
            email_buf: String::new(),
            start_buf: String::new(),
            end_buf: String::new(),
            last_submit: None,
            edit_gen: 0,
            check_run_count: 0,
        };
        c.refresh_buffers(None);
        Some(c)
    }

    // ---- reads (the view derives everything from these, keyed on the version tick) ----------

    /// Shared access to the live core draft, for assertions and power-reads. This IS the
    /// "read the contract directly" claim: no snapshot copy exists anywhere in this shell.
    /// `None` once the handle is a tombstone (submitted or closed).
    pub fn draft(&self) -> Option<Ref<'_, ProfileDraft>> {
        self.handle.borrow()
    }

    /// Read the draft, or fall back. The handle's tombstone state (C17) meets the shell here, and
    /// exactly here: every other read goes through this one function.
    fn with_draft<R: Default>(&self, f: impl FnOnce(&ProfileDraft) -> R) -> R {
        match self.handle.borrow() {
            Some(d) => f(&d),
            None => R::default(),
        }
    }

    /// Mutate the draft, if there is one and it is still editable. A tombstoned handle and an
    /// orphaned draft are both inert — the shell never has to ask which.
    fn edit<R: Default>(&self, f: impl FnOnce(&mut ProfileDraft) -> R) -> R {
        match self.handle.borrow_mut() {
            Some(mut d) if d.status() == DraftStatus::Live => f(&mut d),
            _ => R::default(),
        }
    }

    /// Is there a draft, and is it still attached to a canonical entity?
    pub fn is_live(&self) -> bool {
        self.handle
            .borrow()
            .is_some_and(|d| d.status() == DraftStatus::Live)
    }

    pub fn username_buf(&self) -> &str {
        &self.username_buf
    }

    pub fn name_buf(&self) -> &str {
        &self.name_buf
    }

    pub fn email_buf(&self) -> &str {
        &self.email_buf
    }

    pub fn start_buf(&self) -> &str {
        &self.start_buf
    }

    pub fn end_buf(&self) -> &str {
        &self.end_buf
    }

    pub fn is_dirty(&self, field: ProfileField) -> bool {
        self.with_draft(|d| dirty_of(d, field))
    }

    pub fn any_dirty(&self) -> bool {
        self.with_draft(|d| !d.dirty_fields().is_empty())
    }

    /// The inline error for a field: its tier-1 `Invalid` error, plus (for username) a failed
    /// uniqueness verdict. The full report (required, rules) surfaces on submit.
    pub fn inline_error(&self, field: ProfileField) -> Option<String> {
        let d = self.handle.borrow()?;
        let validity_error: Option<ErrorData> = match field {
            ProfileField::Username => d.username.invalid_error(),
            ProfileField::Name => d.name.invalid_error(),
            ProfileField::Email => d.email.invalid_error(),
            ProfileField::Availability => d.availability.invalid_error(),
        };
        if let Some(e) = validity_error {
            return Some(l10n::message(&e));
        }
        if field == ProfileField::Username
            && let CheckState::Done { verdict: Err(e) } = d.username_check_state()
        {
            return Some(l10n::message(e));
        }
        None
    }

    /// Conflict banner data, if the field is conflicted. Built from `Field` data alone — the §4
    /// claim on trial. The ancestor comes from `Field::base()` now that the core stores it once.
    pub fn conflict(&self, field: ProfileField) -> Option<ConflictInfo> {
        let d = self.handle.borrow()?;
        match field {
            ProfileField::Username => conflict_info(&d.username, |v| v.as_str().to_string()),
            ProfileField::Name => conflict_info(&d.name, |v| v.as_str().to_string()),
            ProfileField::Email => conflict_info(&d.email, |v| v.as_str().to_string()),
            ProfileField::Availability => conflict_info(&d.availability, range_text),
        }
    }

    pub fn conflicts(&self) -> Vec<ProfileField> {
        self.with_draft(|d| d.conflicts())
    }

    /// The async check's core-owned sub-state (cloned): drives the spinner and the C13 asserts.
    pub fn username_check(&self) -> CheckState<Result<(), ErrorData>> {
        match self.handle.borrow() {
            Some(d) => d.username_check_state().clone(),
            None => CheckState::Idle,
        }
    }

    pub fn is_checking(&self) -> bool {
        matches!(self.username_check(), CheckState::Pending { .. })
    }

    pub fn check_run_count(&self) -> u32 {
        self.check_run_count
    }

    pub fn last_submit(&self) -> Option<&SubmitOutcome> {
        self.last_submit.as_ref()
    }

    /// The canonical entity for the simulator pane, via `store.canonical()`.
    pub fn canonical_view(&self) -> Option<CanonicalView> {
        let p = self.store.canonical()?;
        Some(CanonicalView {
            username: p.username.as_str().to_string(),
            name: p.name.as_str().to_string(),
            email: p.email.as_str().to_string(),
            availability: range_text(&p.availability),
        })
    }

    // ---- editing (the echo rule) ------------------------------------------------------------

    pub fn focus(&mut self, field: ProfileField) {
        self.focused = Some(field);
        self.focused_touched = false;
    }

    /// On blur the field is no longer owned by the control, so its buffer refreshes to the
    /// core's sanitized value (or the retained `Invalid.raw` — the user's rejected text stays).
    pub fn blur(&mut self, field: ProfileField) {
        if self.focused == Some(field) {
            self.focused = None;
            self.focused_touched = false;
        }
        self.refresh_buffers(None);
    }

    /// Record a keystroke against the focused control. Only the focused field's buffer is ever
    /// protected, so one flag suffices.
    fn touch(&mut self, field: ProfileField) {
        if self.focused == Some(field) {
            self.focused_touched = true;
        }
    }

    /// Per-keystroke `try_set` — the bet, exercised. The buffer keeps the user's exact text;
    /// the core records `Valid` (sanitized) or `Invalid { raw }` and never touches the buffer.
    /// Returns the debounce ticket for this edit (see [`Self::fire_check_if_current`]).
    pub fn edit_username(&mut self, text: String) -> u64 {
        self.username_buf = text;
        self.touch(ProfileField::Username);
        let raw = self.username_buf.clone();
        self.edit(|d| {
            let _ = d.try_set_username(raw);
        });
        self.edit_gen += 1;
        self.edit_gen
    }

    pub fn edit_name(&mut self, text: String) {
        self.name_buf = text;
        self.touch(ProfileField::Name);
        let raw = self.name_buf.clone();
        self.edit(|d| {
            let _ = d.try_set_name(raw);
        });
    }

    pub fn edit_email(&mut self, text: String) {
        self.email_buf = text;
        self.touch(ProfileField::Email);
        let raw = self.email_buf.clone();
        self.edit(|d| {
            let _ = d.try_set_email(raw);
        });
    }

    pub fn edit_start(&mut self, text: String) {
        self.start_buf = text;
        self.touch(ProfileField::Availability);
        self.try_set_dates();
    }

    pub fn edit_end(&mut self, text: String) {
        self.end_buf = text;
        self.touch(ProfileField::Availability);
        self.try_set_dates();
    }

    /// The grouped setter for the composite value object: both pickers feed one
    /// `try_set_availability(start, end)`. An unparseable buffer (mid-edit / cleared picker) is
    /// a widget state, not a value: skip the set and keep the core's last recorded attempt.
    fn try_set_dates(&mut self) {
        if let (Some(start), Some(end)) = (parse_date(&self.start_buf), parse_date(&self.end_buf)) {
            self.edit(|d| {
                let _ = d.try_set_availability(start, end);
            });
        }
    }

    // ---- async uniqueness check (single-flight; the view owns the timers) --------------------

    /// The debounce timer for `ticket` fired. Begin the check iff no later username edit
    /// superseded it and the username is valid + dirty (nothing worth checking otherwise).
    /// Returns the core's `CheckToken` plus the value to look up; the caller runs the lookup
    /// (async, shell-side) and reports back via [`Self::complete_check`].
    pub fn fire_check_if_current(&mut self, ticket: u64) -> Option<(CheckToken, String)> {
        if ticket != self.edit_gen || !self.is_live() {
            return None;
        }
        let name = {
            let d = self.handle.borrow()?;
            if !d.username.is_dirty() {
                return None;
            }
            d.username.value()?.as_str().to_string()
        };
        let token = self.handle.borrow_mut()?.begin_username_check();
        self.check_run_count += 1;
        Some((token, name))
    }

    /// Deliver a verdict. A stale token (superseded, or reset by a value change — C10/C13) is
    /// discarded by the core; the return says whether the verdict landed.
    pub fn complete_check(&mut self, token: CheckToken, verdict: Result<(), ErrorData>) -> bool {
        self.edit(|d| d.complete_username_check(token, verdict))
    }

    // ---- conflict resolution ------------------------------------------------------------------

    /// A resolution moves the field's value from outside a keystroke, so its buffer refreshes
    /// even if focused (unlike per-keystroke sanitization — the echo rule's one exception).
    pub fn resolve_keep_mine(&mut self, field: ProfileField) {
        self.edit(|d| d.resolve_keep_mine(field));
        self.refresh_buffers(Some(field));
    }

    pub fn resolve_take_theirs(&mut self, field: ProfileField) {
        self.edit(|d| d.resolve_take_theirs(field));
        self.refresh_buffers(Some(field));
    }

    // ---- submit --------------------------------------------------------------------------------

    /// `Store::submit` borrows the handle and leaves a tombstone behind on success (C17), so a
    /// handle living in a struct field submits with no ceremony at all. Before the freeze this
    /// function had to vacate the slot with a throwaway `checkout()` — a real allocation, and a
    /// real registration in the store's rebase list — because `submit` consumed a `!Clone` handle
    /// that could not be moved out from behind `&mut self` (step-04 friction 1).
    ///
    /// On success a fresh `checkout()` starts the next edit session on the new canonical. On
    /// refusal the draft is still there and still live: the user's edits survive (F3).
    pub fn submit(&mut self) {
        match self.store.submit(&mut self.handle) {
            Ok(()) => {
                self.handle = self.store.checkout();
                self.last_submit = Some(SubmitOutcome::Success);
                self.focused = None;
                self.refresh_buffers(None);
            }
            Err(error) => {
                self.last_submit = Some(match error {
                    SubmitError::Validation(report) => SubmitOutcome::Validation(report),
                    SubmitError::Conflicted { fields } => SubmitOutcome::Conflicted(fields),
                    SubmitError::Orphaned => SubmitOutcome::Orphaned,
                    SubmitError::AlreadySubmitted => SubmitOutcome::AlreadySubmitted,
                });
            }
        }
    }

    // ---- server simulator (stands in for a backend) --------------------------------------------

    /// Push a canonical change: the live-rebase / conflict driver. The store mutates the draft
    /// underneath the shell; unfocused buffers refresh from whatever the rebase decided (adopt
    /// keeps them current, conflict keeps *mine* on screen since validity is untouched).
    pub fn sim_set_username(&mut self, raw: &str) {
        if let Some(mut p) = self.store.canonical().cloned()
            && let Ok(u) = Username::try_new(raw.to_string())
        {
            p.username = u;
            self.apply_canonical(p);
        }
    }

    pub fn sim_set_name(&mut self, raw: &str) {
        if let Some(mut p) = self.store.canonical().cloned()
            && let Ok(n) = PersonName::try_new(raw.to_string())
        {
            p.name = n;
            self.apply_canonical(p);
        }
    }

    pub fn sim_set_email(&mut self, raw: &str) {
        if let Some(mut p) = self.store.canonical().cloned()
            && let Ok(e) = Email::try_new(raw.to_string())
        {
            p.email = e;
            self.apply_canonical(p);
        }
    }

    /// Re-apply the seed. Works even after a delete (it is the pane's recovery driver), though
    /// an orphaned draft stays orphaned — rebase skips it, matching the Swift shell.
    pub fn sim_reset_to_seed(&mut self) {
        self.apply_canonical(self.seed.clone());
    }

    /// Delete the canonical entity: every live draft goes `Orphaned`.
    pub fn sim_delete(&mut self) {
        self.store.delete_canonical();
    }

    fn apply_canonical(&mut self, p: Profile) {
        self.store.apply_canonical(p);
        self.refresh_buffers(None);
    }

    // ---- private ---------------------------------------------------------------------------------

    /// Refresh editing buffers from the core.
    ///
    /// The echo rule (§6): the native control owns its text while focused **and typed into**. A
    /// focused field holding live keystrokes keeps its buffer, so core sanitization can never move
    /// the caret. A focused field the user never touched holds nothing worth protecting, so it
    /// adopts a rebase immediately — before the freeze it stayed stale until blur, and the running
    /// app showed the canonical pane and the focused field disagreeing with nothing on screen to
    /// explain it (step-04).
    ///
    /// `force` names a field whose value moved from outside a keystroke (a resolution): refresh it
    /// regardless, and the control is no longer holding anything of the user's.
    fn refresh_buffers(&mut self, force: Option<ProfileField>) {
        let Some((username, name, email, dates)) = self.handle.borrow().map(|d| {
            (
                display(&d.username, |v| v.as_str().to_string()),
                display(&d.name, |v| v.as_str().to_string()),
                display(&d.email, |v| v.as_str().to_string()),
                date_bufs(&d.availability, &self.seed.availability),
            )
        }) else {
            return; // a tombstoned handle has no state to show
        };

        // Only the focused field can be protected, and only while it holds the user's keystrokes.
        let keep_focused = self.focused_touched && force != self.focused;
        let keep = |field: ProfileField| self.focused == Some(field) && keep_focused;

        if !keep(ProfileField::Username) {
            self.username_buf = username;
        }
        if !keep(ProfileField::Name) {
            self.name_buf = name;
        }
        if !keep(ProfileField::Email) {
            self.email_buf = email;
        }
        if !keep(ProfileField::Availability) {
            (self.start_buf, self.end_buf) = dates;
        }
        if !keep_focused {
            // Whatever we just wrote is exactly what the core would render: pristine again.
            self.focused_touched = false;
        }
    }
}

// ---- per-value projection helpers (the monomorphization tax, now on the Rust-shell side) ------

/// Buffer text for a field: the valid value's display, the retained `Invalid.raw`, or empty.
fn display<V, F>(field: &bolted_core::Field<V>, show: F) -> String
where
    V: Value<Raw = String>,
    F: Fn(&V) -> String,
{
    match field.validity() {
        Validity::Valid(v) => show(v),
        Validity::Invalid { raw, .. } => raw.clone(),
        Validity::Unset => String::new(),
    }
}

/// Date-pair buffers from the availability field (`Unset` falls back to the seed, as in Swift).
fn date_bufs(field: &bolted_core::Field<DateRange>, seed: &DateRange) -> (String, String) {
    let (start, end) = match field.validity() {
        Validity::Valid(r) => (r.start(), r.end()),
        Validity::Invalid { raw: (s, e), .. } => (*s, *e),
        Validity::Unset => (seed.start(), seed.end()),
    };
    (fmt_date(start), fmt_date(end))
}

/// Per-field dirtiness. The `match field` fan-out the generator will emit; there is no way around
/// it without reflection (step-04 friction 2, the monomorphization tax).
fn dirty_of(d: &ProfileDraft, field: ProfileField) -> bool {
    match field {
        ProfileField::Username => d.username.is_dirty(),
        ProfileField::Name => d.name.is_dirty(),
        ProfileField::Email => d.email.is_dirty(),
        ProfileField::Availability => d.availability.is_dirty(),
    }
}

/// The 3-way merge data a conflict banner needs: the ancestor (`base`), theirs, and — already on
/// screen — yours. `Field` is the single source; the core stores the ancestor exactly once.
fn conflict_info<V: Value>(field: &Field<V>, show: impl Fn(&V) -> String) -> Option<ConflictInfo> {
    let theirs = field.theirs()?;
    Some(ConflictInfo {
        base: field.base().map(&show),
        theirs: show(theirs),
    })
}

fn range_text(r: &DateRange) -> String {
    format!("{} → {}", fmt_date(r.start()), fmt_date(r.end()))
}

pub fn fmt_date(d: Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year, d.month, d.day)
}

/// Parse the `<input type="date">` wire format. `None` for anything else (mid-edit states).
pub fn parse_date(s: &str) -> Option<Date> {
    let mut parts = s.split('-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(Date::new(year, month, day))
}
