import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

// Consumed IN PLACE from `boltffi pack android`, exactly as the probe does: nothing is vendored, so
// a stale `pack` fails loudly rather than compiling against a checked-in copy.
val ffiDist = file("../../crates/gen-profile-ffi/dist/android")

// `-Pbolted.hw` flips the suite over to the hardware benchmark (`@PhysicalDevice`), which refuses to
// run on an emulator. By default it is excluded, so the headless GMD suite stays green.
val hardwareOnly = providers.gradleProperty("bolted.hw").isPresent

android {
    namespace = "dev.bolted.profileapp"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.bolted.profileapp"
        minSdk = 24
        targetSdk = 35
        versionCode = 1
        versionName = "0.1"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        // One ABI: the app runs on an arm64-v8a emulator or device (see boltffi.toml).
        ndk { abiFilters += "arm64-v8a" }

        val physical = "dev.bolted.profileapp.PhysicalDevice"
        if (hardwareOnly) {
            testInstrumentationRunnerArguments["annotation"] = physical
        } else {
            testInstrumentationRunnerArguments["notAnnotation"] = physical
        }
    }

    sourceSets {
        getByName("main") {
            java.srcDir(ffiDist.resolve("kotlin"))
            jniLibs.srcDir(ffiDist.resolve("jniLibs"))
        }
    }

    buildFeatures { compose = true }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    testOptions {
        managedDevices {
            localDevices {
                // The same headless Gradle-Managed Device the probe uses. Whether a *Compose UI*
                // suite runs here at all is step 07's kill criterion 1 — Android's answer to
                // step 03's XCUITest tier, which needs a GUI session and Accessibility permission.
                create("dev34") {
                    device = "Pixel 2"
                    apiLevel = 34
                    systemImageSource = "aosp-atd"
                }
            }
        }
    }

    packaging {
        resources.excludes += "/META-INF/{AL2.0,LGPL2.1}"
    }
}

kotlin {
    compilerOptions { jvmTarget.set(JvmTarget.JVM_17) }
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.12.01")
    implementation(composeBom)
    androidTestImplementation(composeBom)

    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.material3:material3")
    // `collectAsStateWithLifecycle` — the StateFlow binding that stops collecting in the background.
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    // `SavedStateHandle`: the only container that survives process death. The draft stash lives here.
    implementation("androidx.lifecycle:lifecycle-viewmodel-savedstate:2.8.7")
    // The generated `snapshots(): Flow<ProfileSnapshot>` renders as `callbackFlow { … }`.
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")

    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("androidx.test:core-ktx:1.6.1")
    androidTestImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.9.0")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
}

// Fail early and legibly if `mise run pack:android:gen` has not run.
tasks.withType<com.android.build.gradle.tasks.MergeSourceSetFolders>().configureEach {
    doFirst {
        require(ffiDist.resolve("kotlin").isDirectory && ffiDist.resolve("jniLibs").isDirectory) {
            "missing $ffiDist — run `mise run pack:android:gen` first"
        }
    }
}
