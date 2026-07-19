// Step 26 — the Android HTTP conformance tier. Lives OUTSIDE the cargo workspace (like `apple/` and
// `android/profile-probe`), so `mise run check` stays Rust-only. This is the SIBLING test project
// (the packaging convention from step 25): the consumable adapter is `android/bolted-http`, and it is
// pulled in here as a subproject (its projectDir points at the sibling dir) — the Gradle analog of
// the Apple conformance package's single path dependency on `../bolted-http`.
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
    plugins {
        // Pinned to what this machine has cached (matches profile-probe): AGP 8.7 needs Gradle
        // 8.9–8.11 and JDK 17–21 (mise pins those per-task; see the test:android:http mise task).
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

rootProject.name = "bolted-http-conformance"

// The consumable adapter, included as a sibling subproject.
include(":bolted-http")
project(":bolted-http").projectDir = file("../bolted-http")
