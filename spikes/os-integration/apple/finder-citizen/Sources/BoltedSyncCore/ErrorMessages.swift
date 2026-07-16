// The key → English-template map, step 03's pattern: the shell owns the SENTENCE, the core owns
// the NUMBERS (they arrive as ErrorW params and are substituted into {placeholders}). There must
// be no constraint literal in this file or anywhere under Sources/ — pinned by the greppable
// check in test-os-app.sh, with a planted positive control.

import SyncWireKit

public enum ErrorMessages {
    static let templates: [String: String] = [
        "required": "This field is required.",
        "too_short": "Too short — at least {min} characters (got {actual}).",
        "too_long": "Too long — at most {max} characters (got {actual}).",
        "not_absolute": "Must be an absolute path (starting with /).",
        "interval_out_of_range": "Must be a whole number of minutes within a day.",
        "network_interval_too_fast":
            "Network volumes sync at most every {min} minutes (got {actual}).",
        "folder_check_pending": "Checking that the folder is reachable…",
        "folder_check_required": "The folder needs to be checked before saving.",
        "folder_unreachable": "This folder is not reachable.",
    ]

    public static func render(_ error: ErrorW) -> String {
        guard var text = templates[error.key] else {
            // An unmapped key is still honest data — show it rather than swallow it.
            return error.key
        }
        for (name, value) in error.paramMap {
            text = text.replacingOccurrences(of: "{\(name)}", with: value)
        }
        return text
    }
}
