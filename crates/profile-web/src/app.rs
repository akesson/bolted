//! The Leptos CSR view layer (wasm32 only) — the hand-written stand-in for a generated Leptos
//! shell. It adds only *when* (events, timers, the version tick); every "what" it renders comes
//! from the core through [`ProfileController`]. No constraint literal appears here.
//!
//! **The reactivity pattern.** `bolted_core::Store` and `DraftHandle` are plain, non-reactive
//! Rust: reads go through `handle.borrow()`, and `apply_canonical` mutates the draft *underneath*
//! the shell (live rebase). Leptos cannot see a `borrow()` change, so the shell keeps an explicit
//! reactive tick: [`App::write`] performs the mutation and bumps `version`; [`App::read`]
//! subscribes to `version` before every read. Derived views therefore re-read the live draft on
//! any change — no draft state is ever copied into signals, so nothing forks the core's logic
//! (§4). The only signal-shaped state in this file is the tick itself.

use crate::controller::{
    CanonicalView, ConflictInfo, ProfileController, SubmitOutcome, is_required, max_len,
    simulated_lookup,
};
use crate::l10n;
use gen_profile::ProfileField;
use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;

/// Shell-taste timing constants (the *when*, ARCHITECTURE §2): how long a username edit settles
/// before the uniqueness check fires, and the simulated "server" latency of that check. The wasm
/// test tier shortens both; the core knows about neither.
#[derive(Clone, Copy)]
pub struct Timing {
    pub debounce_ms: u32,
    pub check_latency_ms: u32,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            debounce_ms: 400,
            check_latency_ms: 1000,
        }
    }
}

/// Mount the app to `<body>`. Called by the Trunk bin on load.
pub fn mount(timing: Timing) {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(move || root(timing));
}

/// Mount the app inside `parent`. The wasm test tier uses this to stand up a fresh app per test
/// without clearing `<body>` — which would destroy `wasm-bindgen-test`'s own output node and hang
/// the runner.
pub fn mount_into(parent: leptos::web_sys::HtmlElement, timing: Timing) {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to(parent, move || root(timing)).forget();
}

/// The app's whole state: the controller (which owns the store + the `!Clone` draft handle) and
/// the reactive tick. `StoredValue` is the parking spot a `!Clone`, `!Send` handle needs to be
/// reachable from `'static + Copy` event closures — the ergonomics finding, in one line.
#[derive(Clone, Copy)]
struct App {
    ctrl: StoredValue<ProfileController, LocalStorage>,
    version: RwSignal<u64>,
    timing: Timing,
}

impl App {
    /// Read through the live draft, subscribing to the tick. Every derived view uses this, so a
    /// core-side mutation (including one the shell did not initiate, like a rebase) re-renders.
    fn read<T: Default>(self, f: impl FnOnce(&ProfileController) -> T) -> T {
        self.version.get();
        self.ctrl.try_with_value(f).unwrap_or_default()
    }

    /// Mutate the core, then tick. Every operation that can change the draft goes through here:
    /// `try_set_*`, `resolve_*`, check begin/complete, `apply_canonical`/`delete_canonical`,
    /// `submit`. Forgetting the tick is the one way this shell can go stale — so it is structural.
    fn write<T: Default>(self, f: impl FnOnce(&mut ProfileController) -> T) -> T {
        let out = self.ctrl.try_update_value(f).unwrap_or_default();
        self.version.update(|v| *v = v.wrapping_add(1));
        out
    }

    fn edit(self, field: ProfileField, text: String) {
        match field {
            ProfileField::Username => {
                let ticket = self.write(|c| c.edit_username(text));
                self.schedule_check(ticket);
            }
            ProfileField::Name => self.write(|c| c.edit_name(text)),
            ProfileField::Email => self.write(|c| c.edit_email(text)),
            ProfileField::Availability => {}
        }
    }

    /// The sans-io async check, driven entirely from the browser: debounce (shell taste) → the
    /// core hands out a `CheckToken` (data) → the "server" lookup runs in a `spawn_local` future
    /// the *shell* owns → the verdict goes back to the core. There is no executor in the core.
    ///
    /// Typing through a pending check changes the username's value, which resets the check
    /// (C13); the late `complete_username_check` then carries a stale token and is discarded
    /// (C10). Both fall out of the contract — this function does no bookkeeping for either.
    fn schedule_check(self, ticket: u64) {
        wasm_bindgen_futures::spawn_local(async move {
            TimeoutFuture::new(self.timing.debounce_ms).await;
            // Superseded by a later keystroke, or nothing worth checking → no `begin`, no spinner.
            let Some((token, name)) = self.write(|c| c.fire_check_if_current(ticket)) else {
                return;
            };
            TimeoutFuture::new(self.timing.check_latency_ms).await; // simulated server latency
            let verdict = simulated_lookup(&name);
            self.write(move |c| c.complete_check(token, verdict));
        });
    }
}

fn root(timing: Timing) -> AnyView {
    let Some(controller) = ProfileController::new() else {
        return view! { <main><p>"Failed to initialise the profile store."</p></main> }.into_any();
    };
    let app = App {
        ctrl: StoredValue::new_local(controller),
        version: RwSignal::new(0),
        timing,
    };

    view! {
        <main>
            <section class="editor">
                <h1>"Edit profile"</h1>
                {move || {
                    // Whole-draft status: the base entity was deleted under us (§4). Every setter
                    // is inert from here; what to *do* about it (fail / convert-to-create) is the
                    // app's typed decision — out of scope for the spike, so we just say so.
                    (!app.read(|c| c.is_live()))
                        .then(|| {
                            view! {
                                <p class="err" id="orphan-banner">
                                    "This profile was deleted on the server. Reload to start over."
                                </p>
                            }
                        })
                }}
                {text_row(app, ProfileField::Username, true)}
                {text_row(app, ProfileField::Name, false)}
                {text_row(app, ProfileField::Email, false)}
                {availability_row(app)}
                <div class="actions">
                    <button id="submit" on:click=move |_| app.write(|c| c.submit())>"Submit"</button>
                    {move || {
                        app.read(|c| c.any_dirty())
                            .then(|| view! { <span class="muted">"unsaved changes"</span> })
                    }}
                </div>
                {submit_result(app)}
            </section>
            {simulator_pane(app)}
        </main>
    }
    .into_any()
}

/// One text field: constraint-derived required marker + counter, a dirty dot, the spinner, the
/// inline error, and the conflict banner.
///
/// **The echo rule (§6), in a signal framework.** `value` is a `Memo` over the controller's
/// buffer, and the controller never rewrites the buffer of the *focused* field. So while the user
/// types, the memo's value equals what the DOM input already holds and no write reaches
/// `prop:value` that could move the caret — even though `try_set_*` (and therefore trim/lowercase
/// sanitization, validation, the counter and the debounced check) runs on every keystroke. On
/// blur, or when a value moves from outside a keystroke (rebase adopt, take-theirs, submit), the
/// buffer changes, the memo fires, and the input repaints.
fn text_row(app: App, field: ProfileField, spinner: bool) -> AnyView {
    let value = Memo::new(move |_| app.read(|c| buffer(c, field).to_string()));
    let id = id_str(field);

    view! {
        <div class="row">
            <div class="row-head">
                <label for=format!("field-{id}")>
                    {label(field)}
                    {is_required(field).then_some(" *")}
                </label>
                {move || {
                    app.read(|c| c.is_dirty(field))
                        .then(|| view! { <span class="dot" id=format!("dirty-{id}")>"●"</span> })
                }}
                {spinner
                    .then_some(move || {
                        app.read(|c| c.is_checking())
                            .then(|| {
                                view! {
                                    <span class="spinner" id=format!("spinner-{id}")>
                                        "checking…"
                                    </span>
                                }
                            })
                    })}
                {move || {
                    // The counter's maximum comes from `ProfileField::constraints()`; a field
                    // that declares no length bound (Email) simply has no counter.
                    max_len(field)
                        .map(|max| {
                            let used = value.get().chars().count();
                            view! {
                                <span class="counter" id=format!("counter-{id}")>
                                    {format!("{used}/{max}")}
                                </span>
                            }
                        })
                }}
            </div>
            <input
                type="text"
                id=format!("field-{id}")
                prop:value=move || value.get()
                on:input=move |ev| app.edit(field, event_target_value(&ev))
                on:focus=move |_| app.write(|c| c.focus(field))
                on:blur=move |_| app.write(|c| c.blur(field))
            />
            {inline_error(app, field)} {conflict_banner(app, field)}
        </div>
    }
    .into_any()
}

/// The composite value object's row: two date inputs, one grouped `try_set_availability`.
fn availability_row(app: App) -> AnyView {
    let field = ProfileField::Availability;
    let start = Memo::new(move |_| app.read(|c| c.start_buf().to_string()));
    let end = Memo::new(move |_| app.read(|c| c.end_buf().to_string()));

    view! {
        <div class="row">
            <div class="row-head">
                <label>{label(field)} {is_required(field).then_some(" *")}</label>
                {move || {
                    app.read(|c| c.is_dirty(field))
                        .then(|| view! { <span class="dot" id="dirty-availability">"●"</span> })
                }}
            </div>
            <div class="dates">
                <input
                    type="date"
                    id="field-availability-start"
                    prop:value=move || start.get()
                    on:input=move |ev| app.write(|c| c.edit_start(event_target_value(&ev)))
                    on:focus=move |_| app.write(|c| c.focus(field))
                    on:blur=move |_| app.write(|c| c.blur(field))
                />
                <input
                    type="date"
                    id="field-availability-end"
                    prop:value=move || end.get()
                    on:input=move |ev| app.write(|c| c.edit_end(event_target_value(&ev)))
                    on:focus=move |_| app.write(|c| c.focus(field))
                    on:blur=move |_| app.write(|c| c.blur(field))
                />
            </div>
            {inline_error(app, field)} {conflict_banner(app, field)}
        </div>
    }
    .into_any()
}

fn inline_error(app: App, field: ProfileField) -> impl IntoView {
    move || {
        app.read(|c| c.inline_error(field))
            .map(|text| view! { <p class="err" id=format!("error-{}", id_str(field))>{text}</p> })
    }
}

/// Mine vs theirs (and the common ancestor) straight out of `Field` — the framework's field-level
/// ceiling, rendered. *Mine* needs no display: it is the field's own validity, already in the input
/// above. Since C14 this banner disappears the moment the user types their value.
fn conflict_banner(app: App, field: ProfileField) -> impl IntoView {
    move || {
        app.read(|c| c.conflict(field)).map(|info: ConflictInfo| {
            let id = id_str(field);
            view! {
                <div class="conflict">
                    <p class="conflict-head">"Server changed this field"</p>
                    <p>
                        <span class="muted">"theirs: "</span>
                        <span id=format!("conflict-theirs-{id}")>{info.theirs}</span>
                        {info.base.map(|b| view! { <span class="muted">{format!(" (was {b})")}</span> })}
                    </p>
                    <div class="actions">
                        <button
                            id=format!("keepmine-{id}")
                            on:click=move |_| app.write(|c| c.resolve_keep_mine(field))
                        >
                            "Keep mine"
                        </button>
                        <button
                            id=format!("taketheirs-{id}")
                            on:click=move |_| app.write(|c| c.resolve_take_theirs(field))
                        >
                            "Take theirs"
                        </button>
                    </div>
                </div>
            }
        })
    }
}

/// The submit outcome: a per-field validation report, the conflict refusal, orphan, or success.
/// Every sentence comes from `l10n` keyed on core `ErrorData` — no threshold is restated.
fn submit_result(app: App) -> impl IntoView {
    move || {
        app.read(|c| c.last_submit().cloned())
            .map(|outcome| match outcome {
                SubmitOutcome::Success => {
                    view! { <p class="ok" id="submit-success">"Submitted"</p> }.into_any()
                }
                SubmitOutcome::Validation(report) => {
                    let mut lines: Vec<String> = report
                        .field_errors
                        .iter()
                        .map(|(f, e)| format!("{}: {}", label(*f), l10n::message(e)))
                        .collect();
                    lines.extend(report.rule_errors.iter().map(|r| l10n::message(&r.error)));
                    view! {
                        <div class="err" id="submit-validation">
                            <p>"Fix these before submitting:"</p>
                            <ul>
                                {lines
                                    .into_iter()
                                    .map(|line| view! { <li>{line}</li> })
                                    .collect::<Vec<_>>()}
                            </ul>
                        </div>
                    }
                    .into_any()
                }
                SubmitOutcome::Conflicted(fields) => {
                    let names: Vec<&str> = fields.iter().map(|f| label(*f)).collect();
                    view! {
                        <p class="warn" id="submit-conflicted">
                            {format!("Resolve conflicts: {}", names.join(", "))}
                        </p>
                    }
                    .into_any()
                }
                SubmitOutcome::Orphaned => view! {
                    <p class="err" id="submit-orphaned">
                        {l10n::message(&bolted_core::ErrorData::new("draft_orphaned"))}
                    </p>
                }
                .into_any(),
                // Unreachable here (a successful submit re-checks-out immediately), but the
                // contract has the variant since the handle outlives its draft (C17).
                SubmitOutcome::AlreadySubmitted => view! {
                    <p class="err" id="submit-already">"This edit session has already been submitted."</p>
                }
                .into_any(),
            })
    }
}

/// Stands in for a backend. Shows `store.canonical()` — never the shell's own input echoed back —
/// and drives `apply_canonical` / `delete_canonical`, the live-rebase / conflict / orphan source.
fn simulator_pane(app: App) -> AnyView {
    view! {
        <aside class="simulator">
            <h2>"Server simulator"</h2>
            {move || {
                app.read(|c| c.canonical_view())
                    .map(|c: CanonicalView| {
                        view! {
                            <div class="canonical">
                                <p class="muted">"canonical"</p>
                                <p id="canonical-username">{format!("username: {}", c.username)}</p>
                                <p id="canonical-name">{format!("name: {}", c.name)}</p>
                                <p id="canonical-email">{format!("email: {}", c.email)}</p>
                                <p id="canonical-availability">
                                    {format!("availability: {}", c.availability)}
                                </p>
                            </div>
                        }
                    })
            }}
            <p class="muted">"push a canonical change"</p>
            <button id="sim-username" on:click=move |_| app.write(|c| c.sim_set_username("server_user"))>
                "username → server_user"
            </button>
            <button id="sim-name" on:click=move |_| app.write(|c| c.sim_set_name("Server Name"))>
                "name → Server Name"
            </button>
            <button id="sim-email" on:click=move |_| app.write(|c| c.sim_set_email("team@corp.example"))>
                "email → team@corp.example"
            </button>
            <button id="sim-reset" on:click=move |_| app.write(|c| c.sim_reset_to_seed())>
                "reset to seed"
            </button>
            <button id="sim-delete" on:click=move |_| app.write(|c| c.sim_delete())>
                "delete profile"
            </button>
        </aside>
    }
    .into_any()
}

// ---- per-field projection (the monomorphization tax, on the Rust-shell side) --------------------

fn buffer(c: &ProfileController, field: ProfileField) -> &str {
    match field {
        ProfileField::Username => c.username_buf(),
        ProfileField::Name => c.name_buf(),
        ProfileField::Email => c.email_buf(),
        ProfileField::Availability => c.start_buf(),
    }
}

fn label(field: ProfileField) -> &'static str {
    match field {
        ProfileField::Username => "Username",
        ProfileField::Name => "Name",
        ProfileField::Email => "Email",
        ProfileField::Availability => "Availability",
    }
}

/// Stable, non-localized token for DOM ids, so the headless wasm tier can address elements
/// without depending on display labels (the analog of step-03's accessibility identifiers).
fn id_str(field: ProfileField) -> &'static str {
    match field {
        ProfileField::Username => "username",
        ProfileField::Name => "name",
        ProfileField::Email => "email",
        ProfileField::Availability => "availability",
    }
}
