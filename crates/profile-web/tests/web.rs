//! The headless wasm tier (`mise run test:web`) — it proves the **DOM binding is wired**; the
//! host controller tests own the semantics. Kept lean on purpose.
//!
//! **The contrast worth banking (step doc, Deliverable A):** unlike step-03's XCUITest tier — which
//! needs Xcode, a logged-in GUI session and Accessibility permission, and can therefore never run
//! headless in CI — this suite runs in a headless browser via `wasm-bindgen-test`. Same coverage
//! class (real events into a real render tree), no GUI session.
#![cfg(target_arch = "wasm32")]

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::document;
use profile_web::app::{Timing, mount_into};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
use web_sys::{Element, Event, EventInit, HtmlElement, HtmlInputElement};

wasm_bindgen_test_configure!(run_in_browser);

/// Leptos flushes reactive DOM writes on a later tick, so every assertion follows a yield. (An
/// observation worth recording: the *core* state is correct synchronously — only the paint waits.)
async fn tick() {
    TimeoutFuture::new(20).await;
}

/// Tests share one page, so each mounts a fresh app into its own container. The container is
/// replaced, not `<body>` — clearing `<body>` would delete `wasm-bindgen-test`'s output node and
/// the runner would report "failed to detect test as having been run".
fn fresh_app(timing: Timing) {
    let doc = document();
    if let Some(previous) = doc.get_element_by_id(ROOT) {
        previous.remove();
    }
    let Some(body) = doc.body() else { return };
    let Ok(root) = doc.create_element("div") else {
        return;
    };
    root.set_id(ROOT);
    let _ = body.append_child(&root);
    if let Ok(root) = root.dyn_into::<HtmlElement>() {
        mount_into(root, timing);
    }
}

const ROOT: &str = "test-root";

fn el(id: &str) -> Option<Element> {
    document().get_element_by_id(id)
}

fn text(id: &str) -> Option<String> {
    el(id).map(|e| e.text_content().unwrap_or_default())
}

fn input(id: &str) -> HtmlInputElement {
    el(id)
        .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
        .unwrap_or_else(|| panic!("missing input #{id}"))
}

fn click(id: &str) {
    if let Some(button) = el(id).and_then(|e| e.dyn_into::<HtmlElement>().ok()) {
        button.click();
    }
}

/// Fire a bubbling DOM event (Leptos delegates listeners to the document root).
fn fire(target: &Element, name: &str) {
    let init = EventInit::new();
    init.set_bubbles(true);
    if let Ok(event) = Event::new_with_event_init_dict(name, &init) {
        let _ = target.dispatch_event(&event);
    }
}

/// What a real keystroke does: the browser writes the new text into the control, then fires
/// `input`. The shell must react without writing anything back into the focused control.
fn type_into(id: &str, value: &str) {
    let field = input(id);
    let _ = field.focus();
    fire(&field, "focus");
    field.set_value(value);
    fire(&field, "input");
}

fn blur(id: &str) {
    let field = input(id);
    let _ = field.blur();
    fire(&field, "blur");
}

/// An `input` event runs `try_set_*` through the real core: validity, the constraint-derived
/// counter and the dirty marker all update — while the focused control's own text is left exactly
/// as typed (the echo rule; sanitization lands only on blur).
#[wasm_bindgen_test]
async fn input_event_drives_the_core_without_rewriting_the_focused_buffer() {
    fresh_app(Timing {
        debounce_ms: 100_000, // never fires within the test: this test is about the sync path
        check_latency_ms: 0,
    });
    tick().await;

    assert_eq!(input("field-username").value(), "alice");
    assert_eq!(text("counter-username").as_deref(), Some("5/20"));

    type_into("field-username", "  bob_1  ");
    tick().await;

    // The core parsed and sanitized (the counter and dirty dot prove `try_set` ran)...
    assert_eq!(text("counter-username").as_deref(), Some("9/20"));
    assert!(el("dirty-username").is_some());
    assert!(el("error-username").is_none());
    // ...but the focused control still holds exactly what the user typed. Cursor safety.
    assert_eq!(input("field-username").value(), "  bob_1  ");

    // Blur hands the text back to the core: the sanitized value lands.
    blur("field-username");
    tick().await;
    assert_eq!(input("field-username").value(), "bob_1");
    assert_eq!(text("counter-username").as_deref(), Some("5/20"));
}

/// A rejected input is recorded as `Invalid { raw }`: the core's error sentence renders (with the
/// core's own numbers) and the user's rejected text survives the blur.
#[wasm_bindgen_test]
async fn a_rejected_input_renders_the_core_error_and_keeps_the_raw_text() {
    fresh_app(Timing {
        debounce_ms: 100_000,
        check_latency_ms: 0,
    });
    tick().await;

    type_into("field-username", "ab");
    tick().await;
    assert_eq!(
        text("error-username").as_deref(),
        Some("Too short — minimum 3, got 2.")
    );

    blur("field-username");
    tick().await;
    assert_eq!(input("field-username").value(), "ab");
}

/// A simulator click mutates canonical, the store rebases the live draft *underneath* the shell,
/// and the version tick repaints. A clean field adopts; a dirty one conflicts with mine preserved
/// and the banner rendered from `Field` data alone (theirs from `sync`, the ancestor from `base`).
#[wasm_bindgen_test]
async fn a_simulator_click_rebases_into_the_rendered_fields() {
    fresh_app(Timing::default());
    tick().await;

    // Clean + unfocused → silent adopt.
    click("sim-name");
    tick().await;
    assert_eq!(input("field-name").value(), "Server Name");
    assert!(el("conflict-theirs-name").is_none());
    assert!(el("dirty-name").is_none());

    // Dirty → conflict; the banner shows theirs, the input keeps mine.
    click("sim-reset");
    tick().await;
    type_into("field-email", "me@example.com");
    blur("field-email");
    tick().await;
    click("sim-email");
    tick().await;
    assert_eq!(
        text("conflict-theirs-email").as_deref(),
        Some("team@corp.example")
    );
    assert_eq!(input("field-email").value(), "me@example.com");

    // Take theirs → adopt, clean, banner gone.
    click("taketheirs-email");
    tick().await;
    assert_eq!(input("field-email").value(), "team@corp.example");
    assert!(el("conflict-theirs-email").is_none());
    assert!(el("dirty-email").is_none());
}

/// The sans-io async check, end to end in the browser: a debounced `spawn_local` future begins the
/// check (spinner), a simulated server answers, the core records the verdict, the shell renders it.
/// No executor in the core; the spinner binds to `CheckState::Pending`.
#[wasm_bindgen_test]
async fn the_async_check_shows_and_then_clears_a_spinner() {
    fresh_app(Timing {
        debounce_ms: 30,
        check_latency_ms: 120,
    });
    tick().await;

    type_into("field-username", "admin"); // a taken name
    assert!(el("spinner-username").is_none()); // still inside the debounce window

    TimeoutFuture::new(70).await; // debounce elapsed, verdict still in flight
    assert!(el("spinner-username").is_some());
    assert!(el("error-username").is_none());

    TimeoutFuture::new(150).await; // verdict arrived
    assert!(el("spinner-username").is_none());
    assert_eq!(
        text("error-username").as_deref(),
        Some("That username is already taken.")
    );
}

/// Typing through a pending check changes the checked value, so the core resets the check (C13)
/// and discards the late completion by sequence (C10). The shell does no bookkeeping: the spinner
/// and the verdict simply follow the contract.
#[wasm_bindgen_test]
async fn typing_through_a_pending_check_never_shows_a_verdict_for_the_wrong_text() {
    fresh_app(Timing {
        debounce_ms: 30,
        check_latency_ms: 120,
    });
    tick().await;

    type_into("field-username", "admin");
    TimeoutFuture::new(70).await;
    assert!(el("spinner-username").is_some());

    // One more keystroke, mid-flight. The in-flight verdict now belongs to text that is gone.
    type_into("field-username", "admin2");
    tick().await;
    assert!(el("spinner-username").is_none()); // the check went back to Idle
    assert!(el("error-username").is_none());

    // The original (taken) verdict lands late and is ignored...
    TimeoutFuture::new(150).await;
    assert!(el("error-username").is_none());

    // ...and the check for the *current* text runs and passes on its own schedule.
    TimeoutFuture::new(150).await;
    assert!(el("error-username").is_none());
    assert!(el("spinner-username").is_none());
}

/// Submit, through the DOM: refused while conflicted (and the draft survives — F3 on the real
/// `bolted_core::Store`), then resolved and resubmitted, with final truth arriving via
/// `store.canonical()` rather than the shell's own input echoed back.
#[wasm_bindgen_test]
async fn submit_is_refused_on_conflict_then_succeeds_after_resolution() {
    fresh_app(Timing::default());
    tick().await;

    type_into("field-name", "My Name");
    blur("field-name");
    tick().await;
    click("sim-name");
    tick().await;

    click("submit");
    tick().await;
    assert_eq!(
        text("submit-conflicted").as_deref(),
        Some("Resolve conflicts: Name")
    );
    assert_eq!(input("field-name").value(), "My Name"); // the edit session survived

    click("keepmine-name");
    tick().await;
    click("submit");
    tick().await;
    assert_eq!(text("submit-success").as_deref(), Some("Submitted"));
    assert_eq!(text("canonical-name").as_deref(), Some("name: My Name"));
    assert!(el("dirty-name").is_none()); // re-checked-out: a fresh, clean draft
}

/// An invalid field blocks submit with a per-field report built from core `ErrorData`.
#[wasm_bindgen_test]
async fn submit_renders_the_validation_report() {
    fresh_app(Timing::default());
    tick().await;

    type_into("field-name", "");
    blur("field-name");
    tick().await;
    click("submit");
    tick().await;

    let report = text("submit-validation").unwrap_or_default();
    assert!(
        report.contains("Name: Too short — minimum 1, got 0."),
        "{report}"
    );
    assert!(el("submit-success").is_none());
}

/// Deleting canonical orphans the live draft: a whole-draft status, surfaced as a typed outcome.
#[wasm_bindgen_test]
async fn deleting_canonical_orphans_the_draft() {
    fresh_app(Timing::default());
    tick().await;

    click("sim-delete");
    tick().await;
    assert!(el("orphan-banner").is_some());

    click("submit");
    tick().await;
    assert_eq!(
        text("submit-orphaned").as_deref(),
        Some("This profile was deleted on the server.")
    );
}
