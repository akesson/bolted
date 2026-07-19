import org.jetbrains.kotlin.gradle.dsl.JvmTarget

// `android/bolted-http` — the consumable Android HTTP adapter (step 26, N1). This is the "bundled
// package" analog of `apple/bolted-http`: the hand-written OkHttp adapter (`BoltedHttp.kt`) compiled
// TOGETHER with the generated Kotlin/JNI bindings + the packed `.so`, producing ONE library (AAR)
// a consumer depends on. The sibling `android/bolted-http-conformance` project drives the suite
// against it. Mirrors the proven `pack:android` / profile-probe "consume the dist in place" layout —
// nothing is vendored: if `pack:android:http` drifts, this build fails loudly.
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

// The generated Kotlin + the packed .so are consumed IN PLACE from `boltffi pack android`'s output.
val ffiDist = file("../../crates/bolted-http-android-ffi/dist/android")

android {
    namespace = "dev.bolted.http"
    compileSdk = 35

    defaultConfig {
        minSdk = 24
        // One ABI: the conformance tier runs on an arm64-v8a emulator (see boltffi.toml).
        ndk { abiFilters += "arm64-v8a" }
    }

    sourceSets {
        getByName("main") {
            // The hand-written adapter lives under src/main/kotlin; the generated bindings + jniLibs
            // are pulled straight from the FFI crate's dist (the profile-probe pattern).
            java.srcDir(ffiDist.resolve("kotlin"))
            jniLibs.srcDir(ffiDist.resolve("jniLibs"))
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

kotlin {
    compilerOptions { jvmTarget.set(JvmTarget.JVM_17) }
}

dependencies {
    // OkHttp is the adapter engine (internal to BoltedHttp.kt).
    implementation("com.squareup.okhttp3:okhttp:4.12.0")
    // The generated `chunkStream(): Flow<Chunk>` renders as `callbackFlow { … }`; exposed as `api`
    // so the sibling conformance project can collect the Flow.
    api("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")
}

// Fail early and legibly if `mise run pack:android:http` has not run.
tasks.withType<com.android.build.gradle.tasks.MergeSourceSetFolders>().configureEach {
    doFirst {
        require(ffiDist.resolve("kotlin").isDirectory && ffiDist.resolve("jniLibs").isDirectory) {
            "missing $ffiDist — run `mise run pack:android:http` first"
        }
    }
}
