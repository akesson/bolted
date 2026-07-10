// Step 07 — the Compose spike app, a sibling of `android/profile-probe` and `apple/profile-app`.
// Outside the cargo workspace, so `mise run check` stays Rust-only: no JDK, no Android SDK.
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
    plugins {
        // Pinned to the same versions the probe uses (AGP 8.7 needs Gradle 8.9–8.11 and JDK 17–21).
        id("com.android.application") version "8.7.3"
        id("org.jetbrains.kotlin.android") version "2.1.0"
        // Kotlin 2.x moved the Compose compiler into a first-party plugin, versioned WITH Kotlin.
        id("org.jetbrains.kotlin.plugin.compose") version "2.1.0"
    }
}

dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "profile-app"
