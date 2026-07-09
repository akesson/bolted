// Step 05 — the Android headless probe lives OUTSIDE the cargo workspace (like `apple/`), so
// `mise run check` stays Rust-only: no JDK, no Android SDK, no emulator.
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

rootProject.name = "profile-probe"
