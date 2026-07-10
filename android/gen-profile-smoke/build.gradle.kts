import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

// The generated Kotlin + the packed .so are consumed IN PLACE from `boltffi pack android`'s output,
// exactly as profile-probe consumes the hand-written crate's dist. Nothing is copied or vendored.
val ffiDist = file("../../crates/gen-profile-ffi/dist/android")

android {
    namespace = "dev.bolted.genprofilesmoke"
    compileSdk = 35

    defaultConfig {
        minSdk = 24
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        // One ABI: the smoke runs on an arm64-v8a emulator (see gen-profile-ffi/boltffi.toml).
        ndk { abiFilters += "arm64-v8a" }
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
}

// Fail early and legibly if `mise run pack:android:gen` has not run.
tasks.withType<com.android.build.gradle.tasks.MergeSourceSetFolders>().configureEach {
    doFirst {
        require(ffiDist.resolve("kotlin").isDirectory && ffiDist.resolve("jniLibs").isDirectory) {
            "missing $ffiDist — run `mise run pack:android:gen` first"
        }
    }
}
