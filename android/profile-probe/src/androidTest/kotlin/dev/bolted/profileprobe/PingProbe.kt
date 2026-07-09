package dev.bolted.profileprobe

import com.example.spike_profile_ffi.ProfileStoreFfi
import com.example.spike_profile_ffi.ping
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Milestone 1 — the walking skeleton. Proves the whole pipeline before any probe code exists:
 * `boltffi pack android` → jniLibs + generated Kotlin → Gradle → headless GMD emulator → ART loads
 * `libspike_profile_ffi.so` → JNI_OnLoad → a Rust function returns a String across the boundary.
 */
class PingProbe {
    @Test
    fun pingCrossesTheJniBoundary() {
        assertEquals("pong: hello", ping("hello"))
    }

    @Test
    fun weAreActuallyRunningOnArtNotHotSpot() {
        // Guards the step's central claim. If this ever passes on HotSpot, every number in the
        // report is measuring the wrong runtime.
        val vm = System.getProperty("java.vm.name").orEmpty()
        assertTrue(
            "expected an Android runtime, got java.vm.name='$vm'",
            vm.contains("art", ignoreCase = true) || vm.contains("dalvik", ignoreCase = true),
        )
    }

    @Test
    fun theStoreCrossesAndReportsNoLiveDrafts() {
        ProfileStoreFfi.new().use { store ->
            assertEquals(0u, store.liveDraftCount())
            assertTrue(store.canonical() == null)
        }
    }
}
