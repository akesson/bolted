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
    CheckState, CheckToken, Constraint, Draft, DraftStatus, ErrorData, SubmitError, SyncState,
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
    pub fn draft(&self) -> Ref<'_, ProfileDraft> {
        self.handle.borrow()
    }

    pub fn is_live(&self) -> bool {
        matches!(self.handle.borrow().status(), DraftStatus::Live)
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
        let d = self.handle.borrow();
        match field {
            ProfileField::Username => d.username.is_dirty(),
            ProfileField::Name => d.name.is_dirty(),
            ProfileField::Email => d.email.is_dirty(),
            ProfileField::Availability => d.availability.is_dirty(),
        }
    }

    pub fn any_dirty(&self) -> bool {
        !self.handle.borrow().dirty_fields().is_empty()
    }

    /// The inline error for a field: its tier-1 `Invalid` error, plus (for username) a failed
    /// uniqueness verdict. The full report (required, rules) surfaces on submit.
    pub fn inline_error(&self, field: ProfileField) -> Option<String> {
        let d = self.handle.borrow();
        let validity_error: Option<ErrorData> = match field {
            ProfileField::Username => invalid_error(&d.username),
            ProfileField::Name => invalid_error(&d.name),
            ProfileField::Email => invalid_error(&d.email),
            ProfileField::Availability => invalid_error(&d.availability),
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

    /// Conflict banner data, if the field is conflicted. Built from `Field` data alone
    /// (`SyncState::Conflicted { base, theirs }`) — the §4 claim on trial.
    pub fn conflict(&self, field: ProfileField) -> Option<ConflictInfo> {
        let d = self.handle.borrow();
        match field {
            ProfileField::Username => conflict_info(d.username.sync(), |v| v.as_str().to_string()),
            ProfileField::Name => conflict_info(d.name.sync(), |v| v.as_str().to_string()),
            ProfileField::Email => conflict_info(d.email.sync(), |v| v.as_str().to_string()),
            ProfileField::Availability => conflict_info(d.availability.sync(), range_text),
        }
    }

    pub fn conflicts(&self) -> Vec<ProfileField> {
        self.handle.borrow().conflicts()
    }

    /// The async check's core-owned sub-state (cloned): drives the spinner and the I13 asserts.
    pub fn username_check(&self) -> CheckState<Result<(), ErrorData>> {
        self.handle.borrow().username_check_state().clone()
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
    }

    /// On blur the field is no longer owned by the control, so its buffer refreshes to the
    /// core's sanitized value (or the retained `Invalid.raw` — the user's rejected text stays).
    pub fn blur(&mut self, field: ProfileField) {
        if self.focused == Some(field) {
            self.focused = None;
        }
        self.refresh_buffers(None);
    }

    /// Per-keystroke `try_set` — the bet, exercised. The buffer keeps the user's exact text;
    /// the core records `Valid` (sanitized) or `Invalid { raw }` and never touches the buffer.
    /// Returns the debounce ticket for this edit (see [`Self::fire_check_if_current`]).
    pub fn edit_username(&mut self, text: String) -> u64 {
        self.username_buf = text;
        if self.is_live() {
            let _ = self
                .handle
                .borrow_mut()
                .try_set_username(self.username_buf.clone());
        }
        self.edit_gen += 1;
        self.edit_gen
    }

    pub fn edit_name(&mut self, text: String) {
        self.name_buf = text;
        if self.is_live() {
            let _ = self.handle.borrow_mut().try_set_name(self.name_buf.clone());
        }
    }

    pub fn edit_email(&mut self, text: String) {
        self.email_buf = text;
        if self.is_live() {
            let _ = self
                .handle
                .borrow_mut()
                .try_set_email(self.email_buf.clone());
        }
    }

    pub fn edit_start(&mut self, text: String) {
        self.start_buf = text;
        self.try_set_dates();
    }

    pub fn edit_end(&mut self, text: String) {
        self.end_buf = text;
        self.try_set_dates();
    }

    /// The grouped setter for the composite value object: both pickers feed one
    /// `try_set_availability(start, end)`. An unparseable buffer (mid-edit / cleared picker) is
    /// a widget state, not a value: skip the set and keep the core's last recorded attempt.
    fn try_set_dates(&mut self) {
        if !self.is_live() {
            return;
        }
        if let (Some(start), Some(end)) = (parse_date(&self.start_buf), parse_date(&self.end_buf)) {
            let _ = self.handle.borrow_mut().try_set_availability(start, end);
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
            let d = self.handle.borrow();
            if !d.username.is_dirty() {
                return None;
            }
            d.username.value()?.as_str().to_string()
        };
        let token = self.handle.borrow_mut().begin_username_check();
        self.check_run_count += 1;
        Some((token, name))
    }

    /// Deliver a verdict. A stale token (superseded, or reset by a value change — I10/I13) is
    /// discarded by the core; the return says whether the verdict landed.
    pub fn complete_check(&mut self, token: CheckToken, verdict: Result<(), ErrorData>) -> bool {
        self.handle
            .borrow_mut()
            .complete_username_check(token, verdict)
    }

    // ---- conflict resolution ------------------------------------------------------------------

    /// A resolution moves the field's value from outside a keystroke, so its buffer refreshes
    /// even if focused (unlike per-keystroke sanitization — the echo rule's one exception).
    pub fn resolve_keep_mine(&mut self, field: ProfileField) {
        self.handle.borrow_mut().resolve_keep_mine(field);
        self.refresh_buffers(Some(field));
    }

    pub fn resolve_take_theirs(&mut self, field: ProfileField) {
        self.handle.borrow_mut().resolve_take_theirs(field);
        self.refresh_buffers(Some(field));
    }

    // ---- submit --------------------------------------------------------------------------------

    /// `Store::submit` **consumes** the handle, but the handle lives in a struct field and is
    /// `!Clone` — so it cannot simply be moved out from behind `&mut self`. The slot must be
    /// vacated with something: a scratch `checkout()` (below), or an `Option<ProfileHandle>`
    /// whose `None` is unreachable yet forces every read to handle it (or `expect`, forbidden).
    /// Recorded as the `!Clone`-handle ergonomics finding. On success a real post-commit
    /// `checkout()` replaces the scratch — the next edit session, based on the new canonical. On
    /// refusal the original handle comes back inside the error (F3, on the real
    /// `bolted_core::Store`) and is swapped back: the user's edits survive, the draft stays live.
    pub fn submit(&mut self) {
        let scratch = self.store.checkout();
        let submitted = std::mem::replace(&mut self.handle, scratch);
        match self.store.submit(submitted) {
            Ok(()) => {
                self.handle = self.store.checkout(); // drops the scratch
                self.last_submit = Some(SubmitOutcome::Success);
                self.focused = None;
                self.refresh_buffers(None);
            }
            Err(failure) => {
                self.handle = failure.handle; // drops the scratch
                self.last_submit = Some(match failure.error {
                    SubmitError::Validation(report) => SubmitOutcome::Validation(report),
                    SubmitError::Conflicted { fields } => SubmitOutcome::Conflicted(fields),
                    SubmitError::Orphaned => SubmitOutcome::Orphaned,
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

    /// Refresh editing buffers from the core, skipping the focused field (echo rule) unless
    /// `force` names it (its value moved from outside a keystroke, e.g. a resolution).
    fn refresh_buffers(&mut self, force: Option<ProfileField>) {
        let (username, name, email, dates) = {
            let d = self.handle.borrow();
            (
                display(&d.username, |v| v.as_str().to_string()),
                display(&d.name, |v| v.as_str().to_string()),
                display(&d.email, |v| v.as_str().to_string()),
                date_bufs(&d.availability, &self.seed.availability),
            )
        };
        let skip = |field: ProfileField| self.focused == Some(field) && force != Some(field);
        if !skip(ProfileField::Username) {
            self.username_buf = username;
        }
        if !skip(ProfileField::Name) {
            self.name_buf = name;
        }
        if !skip(ProfileField::Email) {
            self.email_buf = email;
        }
        if !skip(ProfileField::Availability) {
            (self.start_buf, self.end_buf) = dates;
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

/// Tier-1 error of the field's current validity, if `Invalid`. (`Unset` renders on submit as
/// `required` via the report; inline it would nag untouched fields.)
fn invalid_error<V>(field: &bolted_core::Field<V>) -> Option<ErrorData>
where
    V: Value,
    V::Error: Clone + Into<ErrorData>,
{
    match field.validity() {
        Validity::Invalid { error, .. } => Some(error.clone().into()),
        _ => None,
    }
}

fn conflict_info<V: Value>(
    sync: &SyncState<V>,
    show: impl Fn(&V) -> String,
) -> Option<ConflictInfo> {
    match sync {
        SyncState::Conflicted { base, theirs } => Some(ConflictInfo {
            base: base.as_ref().map(&show),
            theirs: show(theirs),
        }),
        SyncState::InSync => None,
    }
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
