package dev.bolted.profileapp

import com.example.spike_profile_ffi.ErrorData

/**
 * The shell owns the *sentence*, never the *numbers*.
 *
 * Every template maps an `ErrorData.key` — the core's stable, language-free identifier — to an
 * English string, with `{param}` placeholders filled from `ErrorData.params`, which the core
 * supplies. That is ARCHITECTURE §1's litmus test made concrete: min/max/expected-domain arrive as
 * data, so no rule threshold is restated here. (`ProfileForm.kt` is where the *constraint* half of
 * the same rule lives: no magic 20, no magic 30.)
 *
 * An unmapped key falls back to the key itself. Step 06 shipped exactly that bug in the Swift shell:
 * `username_check_required` was introduced by C16, no template was added, and the app would have
 * rendered a raw identifier to a user on C16's most common refusal path. `LocalizationCoverageTest`
 * now drives every key the *real core* can produce and fails if any renders as its own key —
 * a per-target check `bolted-check` should own (step-07 report).
 */
object Localization {
    private val templates: Map<String, String> =
        mapOf(
            // tier-1 field validity
            "required" to "Required.",
            "too_short" to "Too short — minimum {min}, got {actual}.",
            "too_long" to "Too long — maximum {max}, got {actual}.",
            "invalid_chars" to "Use only letters, digits and underscore.",
            "invalid_email" to "That is not a valid email address.",
            "range_reversed" to "Start must be on or before end.",
            // tier-2 rule
            "corporate_email_domain" to "A corp_ username needs a {expected} email (got {actual}).",
            // async uniqueness. Two of these are PROGRESS, not failure — see `Localization.isProgress`.
            "username_check_pending" to "Checking availability…",
            "username_check_required" to "Checking that this username is free…",
            "username_taken" to "That username is already taken.",
            // commit-level, shell-supplied
            "draft_orphaned" to "This profile was deleted on the server.",
        )

    /** Render one core error as an English sentence, substituting `{key}` placeholders from params. */
    fun message(error: ErrorData): String {
        var text = templates[error.key] ?: error.key
        for (param in error.params) {
            text = text.replace("{${param.key}}", param.value)
        }
        return text
    }

    /**
     * Is this refusal really *progress*?
     *
     * C16 blocks a submit whose dirty username has no verdict yet. On the first frame after a
     * keystroke — or after a **restore**, where C20 deliberately drops the verdict — that is the
     * normal state of a form the user is still filling in, not a mistake they made. Rendering it in
     * red next to "Too short" would teach users to ignore red. The step-06 report predicted this
     * would bite every shell; it is the one place C16 costs the UI anything.
     */
    fun isProgress(key: String): Boolean =
        key == "username_check_required" || key == "username_check_pending"

    /** Exposed for the coverage test: a key with no template is a defect, not a fallback. */
    internal fun hasTemplate(key: String): Boolean = templates.containsKey(key)
}
