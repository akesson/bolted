// The sync-wire protocol, mirrored in Codable — the shape a generated Swift wire binding would
// take. Values only: raw values, keyed errors, ids, versions; judgements arrive as the core's
// keyed report and are never re-derived here.
//
// Friction worth recording: serde encodes a Rust tuple as a JSON array, so `(String, String)`
// params and `(FieldName, ErrorWire)` report entries need tiny custom unkeyed decoders — a
// generator emitting both sides would pick object shapes instead.

import Foundation

let schemaVersion: UInt32 = 1

/// serde's `(String, String)`: a two-element JSON array.
struct Param: Codable, Equatable {
    let name: String
    let value: String

    init(name: String, value: String) {
        self.name = name
        self.value = value
    }

    init(from decoder: Decoder) throws {
        var c = try decoder.unkeyedContainer()
        name = try c.decode(String.self)
        value = try c.decode(String.self)
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.unkeyedContainer()
        try c.encode(name)
        try c.encode(value)
    }
}

struct ErrorW: Codable, Equatable {
    let key: String
    let params: [Param]
}

/// serde's `(FieldName, ErrorWire)`: a two-element JSON array of mixed types.
struct FieldError: Codable {
    let field: String
    let error: ErrorW

    init(from decoder: Decoder) throws {
        var c = try decoder.unkeyedContainer()
        field = try c.decode(String.self)
        error = try c.decode(ErrorW.self)
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.unkeyedContainer()
        try c.encode(field)
        try c.encode(error)
    }
}

struct RuleW: Codable {
    let rule: String
    let pins: [String]
    let error: ErrorW
}

struct ReportW: Codable {
    let field_errors: [FieldError]
    let rule_errors: [RuleW]
    var isOk: Bool { field_errors.isEmpty && rule_errors.isEmpty }
    var ruleKeys: [String] { rule_errors.map { $0.error.key } }
}

/// The untagged raw: a JSON string or a JSON bool, self-describing.
enum RawW: Codable, Equatable {
    case text(String)
    case flag(Bool)

    init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if let s = try? c.decode(String.self) {
            self = .text(s)
        } else {
            self = .flag(try c.decode(Bool.self))
        }
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .text(let s): try c.encode(s)
        case .flag(let b): try c.encode(b)
        }
    }
}

struct FieldW: Codable {
    let raw: RawW?
    let base: RawW?
    let dirty: Bool
    let theirs: RawW?
}

struct DraftW: Codable {
    let draft: UInt64
    let label: FieldW
    let folder: FieldW
    let interval: FieldW
    let paused: FieldW
    let orphaned: Bool
    let base_version: UInt64
    let report: ReportW
}

struct CanonicalW: Codable {
    let version: UInt64
    let label: String
    let folder: String
    let interval: String
    let paused: Bool
}

// =================================================================================================
// Requests — typed construction, encoded exactly as serde's internally-tagged enums expect.
// =================================================================================================

enum Request {
    case ping
    case version
    case checkout
    case canonicalSnapshot
    case draftSnapshot(draft: UInt64)
    case trySet(draft: UInt64, field: String, value: RawW)
    case validate(draft: UInt64)
    case beginCheck(draft: UInt64, check: String)
    case completeCheck(draft: UInt64, check: String, token: UInt64, ok: Bool)
    case submit(draft: UInt64)
    case togglePaused
}

extension Request: Encodable {
    enum Keys: String, CodingKey {
        case t, draft, field, value, check, token, ok
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: Keys.self)
        switch self {
        case .ping: try c.encode("ping", forKey: .t)
        case .version: try c.encode("version", forKey: .t)
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

struct Resp: Decodable {
    let t: String
    let version: UInt64?
    let draft: UInt64?
    let error: ErrorW?
    let report: ReportW?
    let token: UInt64?
    let accepted: Bool?
    let canonical: CanonicalW?
    let snapshot: DraftW?
    let paused: Bool?
}

struct PushW: Decodable {
    let t: String
    let version: UInt64?
    let draft: UInt64?
    let base_version: UInt64?
}

struct ServerEnv: Decodable {
    let v: UInt32
    let kind: String  // "response" | "push" | "refused"
    let re: UInt64?
    let resp: Resp?
    let push: PushW?
    let reason: String?
}
