package dev.bolted.profileapp

import android.os.Bundle
import android.os.Parcel
import android.util.Log
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.ViewModelStore
import androidx.test.platform.app.InstrumentationRegistry
import com.example.spike_profile_ffi.UniquenessChecker

/** Measurements land in logcat under this tag, the way step 05's probe does. */
const val TAG: String = "BoltedApp"

fun record(label: String, value: String) {
    Log.i(TAG, "$label = $value")
}

/**
 * A `ViewModel` is owned by a `ViewModelStore`, and `onCleared()` is `protected` — so the only honest
 * way for a test to end a ViewModel's life is to clear its store, exactly as a finishing Activity
 * does. That matters here more than usual: `onCleared()` is where the Rust draft is `close()`d, and
 * on ART that is the only path that ever frees it (C18).
 */
class VmHost(
    private val timing: ProfileViewModel.Timing = ProfileViewModel.Timing(debounceMs = 10, checkLatencyMs = 0),
    private val checker: () -> UniquenessChecker = { DefaultChecker() },
) {
    private val store = ViewModelStore()

    fun create(handle: SavedStateHandle = SavedStateHandle()): ProfileViewModel = onMain {
        val factory = object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <T : ViewModel> create(modelClass: Class<T>): T =
                ProfileViewModel(handle, timing, checker) as T
        }
        ViewModelProvider(store, factory)[ProfileViewModel::class.java]
    }

    /** What a finishing Activity does. Triggers `onCleared()`. */
    fun clear() = onMain { store.clear() }
}

/**
 * Run on the main looper and return the result. A `ViewModel` is main-thread state; an instrumented
 * test runs on its own thread, so every VM call in these suites goes through here rather than
 * pretending the threading does not exist.
 */
fun <T> onMain(body: () -> T): T {
    var result: T? = null
    var failure: Throwable? = null
    InstrumentationRegistry.getInstrumentation().runOnMainSync {
        runCatching(body).onSuccess { result = it }.onFailure { failure = it }
    }
    failure?.let { throw it }
    @Suppress("UNCHECKED_CAST")
    return result as T
}

/** Poll until [condition] holds. Stream-delivered snapshots arrive on the main looper, later. */
fun awaitUntil(timeoutMs: Long = 3_000, what: String = "condition", condition: () -> Boolean) {
    val deadline = System.currentTimeMillis() + timeoutMs
    while (System.currentTimeMillis() < deadline) {
        if (onMain(condition)) return
        Thread.sleep(5)
    }
    throw AssertionError("timed out after ${timeoutMs}ms waiting for: $what")
}

/**
 * Push a `Bundle` through a real `Parcel` and read it back.
 *
 * This is what makes the process-death tests worth running. `SavedStateHandle.savedStateProvider()
 * .saveState()` gives us the exact Bundle the framework persists; sending it through a Parcel
 * exercises the exact serialization the framework performs. What no headless test can prove is that
 * Android *chose* to kill us — only that everything downstream of that choice works.
 */
fun parcelRoundTrip(bundle: Bundle): Bundle {
    val parcel = Parcel.obtain()
    return try {
        parcel.writeBundle(bundle)
        parcel.setDataPosition(0)
        parcel.readBundle(ProfileViewModel::class.java.classLoader)
            ?: error("a Bundle that will not survive a Parcel will not survive process death")
    } finally {
        parcel.recycle()
    }
}

/** The Bundle Android would persist for this handle, round-tripped through a Parcel. */
fun SavedStateHandle.persistAndRevive(): SavedStateHandle =
    SavedStateHandle.createHandle(parcelRoundTrip(savedStateProvider().saveState()), null)
