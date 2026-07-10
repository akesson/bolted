import SpikeProfileFfi

/// The shell owns the *sentence*, never the *numbers*. Every template maps an `ErrorData.key` (the
/// core's stable, language-free identifier) to an English string; `{param}` placeholders are filled
/// from `ErrorData.params`, which the core supplies. This is the ARCHITECTURE §2 litmus test made
/// concrete: the constraint values (min/max/expected domain) come from the core, so no rule
/// threshold is restated here. An unmapped key falls back to the key itself (a visible TODO).
public enum Localization {
    static let templates: [String: String] = [
        // tier-1 field validity
        "required": "Required.",
        "too_short": "Too short — minimum {min}, got {actual}.",
        "too_long": "Too long — maximum {max}, got {actual}.",
        "invalid_chars": "Use only letters, digits and underscore.",
        "invalid_email": "That is not a valid email address.",
        "range_reversed": "Start must be on or before end.",
        // tier-2 rule
        "corporate_email_domain": "A corp_ username needs a {expected} email (got {actual}).",
        // async uniqueness
        "username_check_pending": "Checking availability…",
        "username_check_required": "Checking that this username is free…",
        "username_taken": "That username is already taken.",
        // commit-level. `field_conflicted` is gone: since the freeze, a conflicted or orphaned draft
        // is refused with a typed `SubmitError` variant, not a synthetic rule violation stuffed into
        // a `ValidationReport` (step-01 F5).
        "draft_orphaned": "This profile was deleted on the server.",
    ]

    /// Render one core error as an English sentence, substituting `{key}` placeholders from params.
    public static func message(_ error: ErrorData) -> String {
        var text = templates[error.key] ?? error.key
        for param in error.params {
            text = text.replacingOccurrences(of: "{\(param.key)}", with: param.value)
        }
        return text
    }
}
