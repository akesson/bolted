// The sync-wire protocol, mirrored in Codable — copied from sync-probe (step 18) and extended
// with the verbs a real UI needs: resolve, close, stash, restore, stats. Values only: raw values,
// keyed errors, ids, versions; judgements arrive as the core's keyed report and are never
// re-derived here (the founding rule, on trial over the wire — step-19 kill 3).
//
// Friction carried over from step 18: serde encodes Rust tuples as JSON arrays, so
// `(String, String)` params and `(FieldName, ErrorWire)` report entries need tiny custom unkeyed
// decoders — a generator emitting both sides would pick object shapes instead.

import Foundation

public let schemaVersion: UInt32 = 1

/// serde's `(String, String)`: a two-element JSON array.
public struct Param: Codable, Equatable {
    public let name: String
    public let value: String

    public init(name: String, value: String) {
        self.name = name
        self.value = value
    }

    public init(from decoder: Decoder) throws {
        var c = try decoder.unkeyedContainer()
        name = try c.decode(String.self)
        value = try c.decode(String.self)
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.unkeyedContainer()
        try c.encode(name)
        try c.encode(value)
    }
}

public struct ErrorW: Codable, Equatable {
    public let key: String
    public let params: [Param]

    /// Params as a lookup — the UI renders `key → template` sentences with these values.
    public var paramMap: [String: String] {
        Dictionary(params.map { ($0.name, $0.value) }, uniquingKeysWith: { a, _ in a })
    }
}

/// serde's `(FieldName, ErrorWire)`: a two-element JSON array of mixed types.
public struct FieldError: Codable {
    public let field: String
    public let error: ErrorW

    public init(from decoder: Decoder) throws {
        var c = try decoder.unkeyedContainer()
        field = try c.decode(String.self)
        error = try c.decode(ErrorW.self)
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.unkeyedContainer()
        try c.encode(field)
        try c.encode(error)
    }
}

public struct RuleW: Codable {
    public let rule: String
    public let pins: [String]
    public let error: ErrorW
}

public struct ReportW: Codable {
    public let field_errors: [FieldError]
    public let rule_errors: [RuleW]
    public var isOk: Bool { field_errors.isEmpty && rule_errors.isEmpty }
    public var ruleKeys: [String] { rule_errors.map { $0.error.key } }

    /// Field errors for one field — what the UI hangs under each control.
    public func errors(for field: String) -> [ErrorW] {
        field_errors.filter { $0.field == field }.map { $0.error }
            + rule_errors.filter { $0.pins.contains(field) }.map { $0.error }
    }
}

/// The untagged raw: a JSON string or a JSON bool, self-describing.
public enum RawW: Codable, Equatable {
    case text(String)
    case flag(Bool)

    public init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if let s = try? c.decode(String.self) {
            self = .text(s)
        } else {
            self = .flag(try c.decode(Bool.self))
        }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .text(let s): try c.encode(s)
        case .flag(let b): try c.encode(b)
        }
    }

    public var asText: String? {
        if case .text(let s) = self { return s }
        return nil
    }
}

public struct FieldW: Codable {
    public let raw: RawW?
    public let base: RawW?
    public let dirty: Bool
    public let theirs: RawW?

    /// Conflicted = the rebase preserved mine while theirs moved (values-only reading: the
    /// daemon sends `theirs` only for conflicted fields — no judgement re-derived here).
    public var conflicted: Bool { theirs != nil }
}

public struct DraftW: Codable {
    public let draft: UInt64
    public let label: FieldW
    public let folder: FieldW
    public let interval: FieldW
    public let paused: FieldW
    public let orphaned: Bool
    public let base_version: UInt64
    public let report: ReportW

    public func field(_ name: String) -> FieldW? {
        switch name {
        case "label": return label
        case "folder": return folder
        case "interval": return interval
        case "paused": return paused
        default: return nil
        }
    }
}

public struct CanonicalW: Codable, Equatable {
    public let version: UInt64
    public let label: String
    public let folder: String
    public let interval: String
    public let paused: Bool
}

/// One field of a stash: `{raw, base}` in raw form (C20 — no sync state, no verdict). Round-trips
/// daemon → client → (a possibly different) daemon verbatim; the client never constructs one.
public struct FieldStashW: Codable {
    public let raw: RawW?
    public let base: RawW?
}

/// The H6 survival blob — the one frame that lives *outside* the envelope, as a client-kept
/// value between daemons (step-18 friction 6).
public struct StashW: Codable {
    public let label: FieldStashW
    public let folder: FieldStashW
    public let interval: FieldStashW
    public let paused: FieldStashW
    public let base_version: UInt64
    public let orphaned: Bool
}

// =================================================================================================
// Requests — typed construction, encoded exactly as serde's internally-tagged enums expect.
// =================================================================================================

public enum Request {
    case ping
    case version
    case stats
    case checkout
    case canonicalSnapshot
    case draftSnapshot(draft: UInt64)
    case trySet(draft: UInt64, field: String, value: RawW)
    case validate(draft: UInt64)
    case resolve(draft: UInt64, field: String, keepMine: Bool)
    case beginCheck(draft: UInt64, check: String)
    case completeCheck(draft: UInt64, check: String, token: UInt64, ok: Bool)
    case submit(draft: UInt64)
    case close(draft: UInt64)
    case stash(draft: UInt64)
    case restore(stash: StashW)
    case togglePaused
}

extension Request: Encodable {
    enum Keys: String, CodingKey {
        case t, draft, field, value, check, token, ok, keep_mine, stash
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: Keys.self)
        switch self {
        case .ping: try c.encode("ping", forKey: .t)
        case .version: try c.encode("version", forKey: .t)
        case .stats: try c.encode("stats", forKey: .t)
        case .checkout: try c.encode("checkout", forKey: .t)
        case .canonicalSnapshot: try c.encode("canonical_snapshot", forKey: .t)
        case .draftSnapshot(let draft):
            try c.encode("draft_snapshot", forKey: .t)
            try c.encode(draft, forKey: .draft)
        case .trySet(let draft, let field, let value):
            try c.encode("try_set", forKey: .t)
            try c.encode(draft, forKey: .draft)
            try c.encode(field, forKey: .field)
            try c.encode(value, forKey: .value)
        case .validate(let draft):
            try c.encode("validate", forKey: .t)
            try c.encode(draft, forKey: .draft)
        case .resolve(let draft, let field, let keepMine):
            try c.encode("resolve", forKey: .t)
            try c.encode(draft, forKey: .draft)
            try c.encode(field, forKey: .field)
            try c.encode(keepMine, forKey: .keep_mine)
        case .beginCheck(let draft, let check):
            try c.encode("begin_check", forKey: .t)
            try c.encode(draft, forKey: .draft)
            try c.encode(check, forKey: .check)
        case .completeCheck(let draft, let check, let token, let ok):
            try c.encode("complete_check", forKey: .t)
            try c.encode(draft, forKey: .draft)
            try c.encode(check, forKey: .check)
            try c.encode(token, forKey: .token)
            try c.encode(ok, forKey: .ok)
        case .submit(let draft):
            try c.encode("submit", forKey: .t)
            try c.encode(draft, forKey: .draft)
        case .close(let draft):
            try c.encode("close", forKey: .t)
            try c.encode(draft, forKey: .draft)
        case .stash(let draft):
            try c.encode("stash", forKey: .t)
            try c.encode(draft, forKey: .draft)
        case .restore(let stash):
            try c.encode("restore", forKey: .t)
            try c.encode(stash, forKey: .stash)
        case .togglePaused:
            try c.encode("toggle_paused", forKey: .t)
        }
    }
}

struct ClientFrame: Encodable {
    let v: UInt32
    let seq: UInt64
    let req: Request
}

// =================================================================================================
// Server frames — a flat pragmatic decode: `t` discriminates, payload fields are optional.
// =================================================================================================

/// A submit/toggle refusal, flat-decoded: `kind` discriminates, payloads are optional.
public struct RefusalW: Decodable {
    public let kind: String  // validation | conflicted | orphaned | already_submitted | no_canonical
    public let report: ReportW?
    public let fields: [String]?
}

public struct Resp: Decodable {
    public let t: String
    public let version: UInt64?
    public let draft: UInt64?
    public let drafts: UInt64?
    public let rebasing: UInt64?
    public let error: ErrorW?
    public let report: ReportW?
    public let token: UInt64?
    public let accepted: Bool?
    public let canonical: CanonicalW?
    public let snapshot: DraftW?
    public let paused: Bool?
    public let stash: StashW?
    public let refusal: RefusalW?
}

public struct PushW: Decodable {
    public let t: String
    public let version: UInt64?
    public let draft: UInt64?
    public let base_version: UInt64?
}

struct ServerEnv: Decodable {
    let v: UInt32
    let kind: String  // "response" | "push" | "refused"
    let re: UInt64?
    let resp: Resp?
    let push: PushW?
    let reason: String?
}
