//! The Leptos CSR view layer (wasm32 only). Milestone 1: the smallest honest proof — a seeded
//! `ProfileStore`, a real `checkout()`, and one field's value rendered from `handle.borrow()`.
//! The full form, the version-tick reactivity, and the simulator pane land in milestones 2–4.

use crate::controller::seeded_store;
use leptos::prelude::*;

/// Shell-taste timing constants (the *when*, ARCHITECTURE §2): how long a username edit settles
/// before the uniqueness check fires, and the simulated "server" latency of that check. The wasm
/// test tier shortens both; the constants live here, not in the core.
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

/// Mount the app to `<body>`. Called by the trunk bin on load and by the wasm test tier.
pub fn mount(timing: Timing) {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(move || root(timing));
}

fn root(_timing: Timing) -> impl IntoView {
    // The store and the draft handle live for the app's lifetime. `DraftHandle` is `!Clone` by
    // design; milestone 1 only reads once at mount, so plain ownership suffices here — parking
    // it for event closures is the milestone-2 crux.
    let Some(mut store) = seeded_store() else {
        return view! { <main><p>"Failed to initialise the profile store."</p></main> }.into_any();
    };
    let handle = store.checkout();
    let username = handle
        .borrow()
        .username
        .value()
        .map(|u| u.as_str().to_string())
        .unwrap_or_default();

    view! {
        <main>
            <h1>"Bolted — Profile Spike (Leptos)"</h1>
            <p>"username (read via handle.borrow()): " <strong id="m1-username">{username}</strong></p>
        </main>
    }
    .into_any()
}
