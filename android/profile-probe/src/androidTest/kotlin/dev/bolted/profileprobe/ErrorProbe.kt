package dev.bolted.profileprobe

import com.example.gen_profile_ffi.ProfileDraftFfi
import com.example.gen_profile_ffi.ProfileFieldId
import com.example.gen_profile_ffi.ProfileStoreFfi
import com.example.gen_profile_ffi.SubmitErrorFfi
import com.example.gen_profile_ffi.UsernameErrorFfi
import com.example.gen_profile_ffi.TextValidity
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Test

/**
 * Probe matrix D — typed errors on the Kotlin backend (`error_style = "throwing"`).
 *
 * Step 02 asserted exactly this in Swift. It must hold across a *different codegen backend* or the
 * errors-as-data decision (ARCHITECTURE §8, "errors are key+params data, never strings") is a
 * property of the Swift generator rather than of BoltFFI.
 */
class ErrorProbe {
    private lateinit var store: ProfileStoreFfi
    private lateinit var draft: ProfileDraftFfi

    @Before
    fun setUp() {
        store = seededStore()
        draft = store.checkout()
    }

    @After
    fun tearDown() {
        draft.close()
        store.close()
    }

    /** A tier-1 field error crosses as a payload-carrying case, read structurally. */
    @Test
    fun aRejectedSetterThrowsATypedErrorWithReadableParams() {
        try {
            draft.trySetUsername("ab")
            fail("expected UsernameErrorFfi.TooShort")
        } catch (e: UsernameErrorFfi.TooShort) {
            assertEquals("the core's own minimum, not a shell literal", 3u, e.min)
            assertEquals(2u, e.actual)
            record("error.too_short", "min=${e.min} actual=${e.actual}")
        }
    }

    /** The rejected raw text survives as `Invalid { raw }`, so the shell can echo it back. */
    @Test
    fun aRejectedValueIsRecordedAsInvalidWithTheRawText() {
        runCatching { draft.trySetUsername("ab") }
        val validity = draft.snapshot().username.validity
        assertTrue("expected Invalid, got $validity", validity is TextValidity.Invalid)
        assertEquals("ab", (validity as TextValidity.Invalid).raw)
    }

    /** A refused submit carries the whole structured report, not a flattened message. */
    @Test
    fun submitThrowsValidationCarryingTheNestedReport() {
        runCatching { draft.trySetName("") } // rejected -> the field is now Invalid
        try {
            draft.submit()
            fail("expected SubmitErrorFfi.Validation")
        } catch (e: SubmitErrorFfi.Validation) {
            val nameError = e.report.fieldErrors.single { it.field == ProfileFieldId.NAME }
            assertEquals("too_short", nameError.error.key)
            val params = nameError.error.params.associate { it.key to it.value }
            assertEquals("1", params["min"])
            assertEquals("0", params["actual"])
            record("error.submit_report", "key=${nameError.error.key} params=$params")
        }
    }

    /**
     * The tier-2 relational rule reaches Kotlin as a rule violation with its pins — the `corp_`
     * username requires the `corp.example` domain, and the seed's email does not match.
     */
    @Test
    fun aTier2RuleViolationCrossesWithItsPinsAndParams() {
        draft.trySetUsername("corp_alice")
        // C16: a dirty username with an unrun check is itself a rule violation, so without this the
        // report would carry two and the tier-2 rule under test would not be `single()`.
        draft.setUsernameChecker(uniqueChecker())
        draft.runUsernameCheck()
        try {
            draft.submit()
            fail("expected SubmitErrorFfi.Validation carrying a rule violation")
        } catch (e: SubmitErrorFfi.Validation) {
            val violation = e.report.ruleErrors.single()
            assertEquals("corporate_email", violation.rule)
            assertEquals(listOf(ProfileFieldId.EMAIL), violation.pins)
            assertEquals("corporate_email_domain", violation.error.key)
            val params = violation.error.params.associate { it.key to it.value }
            assertEquals("corp.example", params["expected"])
            assertEquals("example.com", params["actual"])
            record("error.tier2_rule", "${violation.rule} pins=${violation.pins} params=$params")
        }
    }

    /** A second submit on a consumed draft is a typed lifecycle error, not a crash. */
    @Test
    fun submittingTwiceYieldsAlreadySubmitted() {
        draft.submit()
        try {
            draft.submit()
            fail("expected SubmitErrorFfi.AlreadySubmitted")
        } catch (e: SubmitErrorFfi) {
            assertTrue("got $e", e is SubmitErrorFfi.AlreadySubmitted)
        }
    }
}
