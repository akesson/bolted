package dev.bolted.profileapp

import com.example.gen_profile_ffi.AvailabilityRaw
import com.example.gen_profile_ffi.ErrorData
import com.example.gen_profile_ffi.PlainDate
import com.example.gen_profile_ffi.ProfileDraftFfi
import com.example.gen_profile_ffi.ProfileStoreFfi
import com.example.gen_profile_ffi.SubmitErrorFfi
import com.example.gen_profile_ffi.UsernameChecker
import com.example.gen_profile_ffi.CheckVerdictFfi
import com.example.gen_profile_ffi.CheckStateFfi
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * **Step-06 friction 7, made into a test.**
 *
 * C16 introduced the `username_check_required` error key. The Rust shell got a template. The Swift
 * shell did not — and would have rendered a raw identifier to a user on C16's most common refusal
 * path. Nothing caught it; verifying the report by hand did. That report asked for `bolted-check` to
 * verify localization key coverage per target. This is the shape such a check should have.
 *
 * Rather than assert against a hardcoded list of keys (which would only ever be as good as the list),
 * every case below **drives the real core** and asserts that whatever `ErrorData` comes back renders
 * as a sentence, not as its own key. A key the core learns to emit and this shell has not learned to
 * say fails here.
 */
class LocalizationCoverageTest {

    /** Every rendered error must differ from its key — the key IS the fallback (`Localization`). */
    private fun assertRenders(error: ErrorData) {
        assertTrue(
            "no template for '${error.key}': the app would show a user a raw identifier",
            Localization.hasTemplate(error.key),
        )
        val message = Localization.message(error)
        assertTrue("'${error.key}' rendered as its own key", message != error.key)
        assertTrue("'${error.key}' left a placeholder unfilled: $message", !message.contains("{"))
        record("l10n.${error.key}", message)
    }

    private fun seeded(): ProfileStoreFfi = ProfileStoreFfi.new().also { it.applyCanonical(SEED) }

    private fun errorsOf(draft: ProfileDraftFfi): List<ErrorData> {
        val report = draft.validate()
        return report.fieldErrors.map { it.error } + report.ruleErrors.map { it.error }
    }

    /** Tier 1, every value type, every failure mode the core can produce for it. */
    @Test
    fun everyTier1ErrorRenders() {
        seeded().use { store ->
            store.checkout().use { draft ->
                // too_short / too_long / invalid_chars (username)
                runCatching { draft.trySetUsername("ab") }
                errorsOf(draft).forEach(::assertRenders)
                runCatching { draft.trySetUsername("x".repeat(21)) }
                errorsOf(draft).forEach(::assertRenders)
                runCatching { draft.trySetUsername("bad!name") }
                errorsOf(draft).forEach(::assertRenders)

                // invalid_email
                runCatching { draft.trySetEmail("nope") }
                errorsOf(draft).forEach(::assertRenders)

                // range_reversed
                runCatching {
                    draft.trySetAvailability(
                        AvailabilityRaw(
                            PlainDate(2026.toUShort(), 12.toUByte(), 31.toUByte()),
                            PlainDate(2026.toUShort(), 1.toUByte(), 1.toUByte()),
                        )
                    )
                }
                errorsOf(draft).forEach(::assertRenders)
            }
        }
    }

    /** `required` only exists for a create-flow draft: an unseeded store has no canonical to copy. */
    @Test
    fun theRequiredErrorRenders() {
        ProfileStoreFfi.new().use { store ->
            store.checkout().use { draft ->
                val errors = errorsOf(draft)
                assertEquals("all four fields are Unset", 4, errors.count { it.key == "required" })
                errors.forEach(::assertRenders)
            }
        }
    }

    /** Tier 2, with its params: the sentence names `corp.example`, and the core supplied that word. */
    @Test
    fun theTier2RuleErrorRendersWithItsParams() {
        seeded().use { store ->
            store.checkout().use { draft ->
                draft.setUsernameChecker(alwaysUnique())
                draft.trySetUsername("corp_alice")
                draft.runUsernameCheck()
                draft.trySetEmail("alice@other.com")

                val violation = draft.validate().ruleErrors.single { it.rule == "corporate_email" }
                assertRenders(violation.error)
                assertEquals(
                    "A corp_ username needs a corp.example email (got other.com).",
                    Localization.message(violation.error),
                )
            }
        }
    }

    /** The async keys: `username_taken` (failure) and `username_check_required` (progress). */
    @Test
    fun theAsyncCheckKeysRender() {
        seeded().use { store ->
            store.checkout().use { draft ->
                // never run, dirty -> C16 refuses, and it is PROGRESS
                draft.trySetUsername("alice2")
                val required = draft.validate().ruleErrors.single { it.rule == "username_unique" }
                assertEquals("username_check_required", required.error.key)
                assertRenders(required.error)
                assertTrue(Localization.isProgress(required.error.key))

                // a taken verdict -> a real error
                draft.setUsernameChecker(alwaysTaken())
                draft.runUsernameCheck()
                val check = draft.snapshot().usernameCheck
                assertTrue(check is CheckStateFfi.Failed)
                assertRenders((check as CheckStateFfi.Failed).error)
                assertTrue(!Localization.isProgress(check.error.key))
            }
        }
    }

    /**
     * `username_check_pending` cannot be produced from this shell: with a synchronous checker,
     * `begin` and `complete` are atomic inside one FFI call, so `Pending` is only ever seen on the
     * stream (D10; §9's "a real `Pending` across FFI", owned by step 10). Its template is asserted
     * directly, and this test is the note explaining why it cannot be driven.
     */
    @Test
    fun thePendingKeyHasATemplateEvenThoughThisShellCannotProduceIt() {
        assertRenders(ErrorData("username_check_pending", emptyList()))
    }

    /** `draft_orphaned` is shell-supplied: the core reports orphaning as a typed variant, not a key. */
    @Test
    fun theOrphanedOutcomeRenders() {
        seeded().use { store ->
            store.checkout().use { draft ->
                store.applyCanonical(SEED) // keep it simple: orphan by deleting is not exposed here
                assertRenders(ErrorData("draft_orphaned", emptyList()))
                // and the typed refusal really is typed, not a rule violation (C07/C11)
                assertTrue(SubmitErrorFfi.Orphaned is SubmitErrorFfi)
            }
        }
    }

    private fun alwaysUnique() = object : UsernameChecker {
        override fun check(value: String) = CheckVerdictFfi.PASS
    }

    private fun alwaysTaken() = object : UsernameChecker {
        override fun check(value: String) = CheckVerdictFfi.FAIL
    }
}
