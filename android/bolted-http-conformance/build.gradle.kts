import org.jetbrains.kotlin.gradle.dsl.JvmTarget

// `android/bolted-http-conformance` — the instrumented ART conformance tier for the Android HTTP
// adapter (step 26). Drives the `bolted-http` conformance suite through the JNI `HttpHarness` on a
// headless Gradle-Managed Device (`dev34`, aosp_atd android-34 arm64 — the same recipe as
// `test:android`). Depends on the sibling consumable `:bolted-http` (adapter + generated bindings +
// packed .so) as its ONE dependency.
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "dev.bolted.http.conformance"
    compileSdk = 35

    defaultConfig {
        minSdk = 24
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        // One ABI: the conformance tier runs on an arm64-v8a emulator (matches the packed .so).
        ndk { abiFilters += "arm64-v8a" }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    testOptions {
        managedDevices {
            localDevices {
                // Headless — no GUI session, no Accessibility permission (the test:android recipe).
                create("dev34") {
                    device = "Pixel 2"
                    apiLevel = 34
                    systemImageSource = "aosp-atd"
                }
            }
        }
    }
}

kotlin {
    compilerOptions { jvmTarget.set(JvmTarget.JVM_17) }
}

dependencies {
    // The consumable adapter (+ generated bindings + packed .so), the single dependency.
    androidTestImplementation(project(":bolted-http"))

    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")
    androidTestImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.9.0")
    // The N2 stream probe drives `/chunked` with OkHttp directly (a test-tier producer), independent
    // of the adapter under test — so the test target needs OkHttp on its own classpath.
    androidTestImplementation("com.squareup.okhttp3:okhttp:4.12.0")
}
