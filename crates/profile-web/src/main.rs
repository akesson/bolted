//! Trunk's bin entry point. On wasm it mounts the Leptos app; on the host (workspace `check`)
//! it compiles to an empty main so `cargo clippy/test --workspace` stay host-only and Leptos-free.

fn main() {
    #[cfg(target_arch = "wasm32")]
    profile_web::app::mount(profile_web::app::Timing::default());
}
