//! The framework-light controller over `ProfileStore` + `ProfileHandle` — the analog of
//! step-03's `ProfileViewModel`, which ran headless. The four behaviours are tested at this
//! level with plain `cargo test`; the Leptos layer above adds only *when* (events, debounce
//! timers, the version tick), never *what*.
//!
//! Milestone 1: seed + store construction only (enough for the skeleton app to check out a
//! draft and render a field from `handle.borrow()`). The editing surface lands in milestone 2.

use bolted_core::Value;
use spike_profile::{Date, DateRange, Email, PersonName, Profile, ProfileStore, Username};

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

/// A store seeded with [`seed_profile`], or `None` if the seed failed to validate.
pub fn seeded_store() -> Option<ProfileStore> {
    Some(ProfileStore::new(Some(seed_profile()?)))
}
