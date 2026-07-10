import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

// The generated Kotlin + the packed .so are consumed IN PLACE from `boltffi pack android`'s output.
// Nothing is copied or vendored: if `pack` drifts, this project must fail loudly rather than compile
// against a stale checked-in copy.
val ffiDist = file("../../crates/gen-profile-ffi/dist/android")

// `-Pbolted.hazard` flips the suite over to the UB probes (H2), which may kill the instrumented
// process. By default they are excluded so a native crash cannot destroy the other probes' results.
val hazardOnly = providers.gradleProperty("bolted.hazard").isPresent

android {
    namespace = "dev.bolted.profileprobe"
    compileSdk = 35

    defaultConfig {
        minSdk = 24
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        // One ABI: the probe runs on an arm64-v8a emulator (see boltffi.toml).
        ndk { abiFilters += "arm64-v8a" }

        val hazard = "dev.bolted.profileprobe.HazardProbe"
        if (hazardOnly) {
            testInstrumentationRunnerArguments["annotation"] = hazard
        } else {
            testInstrumentationRunnerArguments["notAnnotation"] = hazard
        }
    }

    sourceSets {
        getByName("main") {
            java.srcDir(ffiDist.resolve("kotlin"))
            jniLibs.srcDir(ffiDist.resolve("jniLibs"))
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    testOptions {
        managedDevices {
            localDevices {
                // Headless by default — no GUI session, no Accessibility permission. This is the
                // whole point of "headless probe", and the contrast with step 03's XCUITest tier.
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
    // The generated `snapshots(): Flow<ProfileSnapshot>` renders as `callbackFlow { … }`.
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")

    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.9.0")
}

// Fail early and legibly if `mise run pack:android:gen` has not run.
tasks.withType<com.android.build.gradle.tasks.MergeSourceSetFolders>().configureEach {
    doFirst {
        require(ffiDist.resolve("kotlin").isDirectory && ffiDist.resolve("jniLibs").isDirectory) {
            "missing $ffiDist — run `mise run pack:android:gen` first"
        }
    }
}
