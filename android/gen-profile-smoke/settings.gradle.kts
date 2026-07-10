// Step 11 (M0) — the Kotlin twin of `apple/gen-profile-smoke`: proves `boltffi pack android`
// works on a GENERATED crate and that the result loads on ART. Outside the cargo workspace, like
// everything under `android/`, so `mise run check` stays Rust-only.
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
    plugins {
        // Pinned to what this machine has cached. AGP 8.7 requires Gradle 8.9–8.11 and JDK 17+
        // (it rejects the Homebrew default JDK 26 — see mise's `test:android` doctor check).
        id("com.android.library") version "8.7.3"
        id("org.jetbrains.kotlin.android") version "2.1.0"
    }
}

dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "gen-profile-smoke"
