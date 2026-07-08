import Foundation
import Observation
import SpikeProfileFfi

/// The hand-written stand-in for the ViewModel a shell generator (step 10/11) would emit. It owns a
/// draft checkout and translates between the core's snapshot contract and SwiftUI, adding only
/// *when* (debounce, focus, the echo rule's deferral), never *what* (no constraint or rule value is
/// restated — those come from the core via `constraints()` and `ErrorData`).
///
/// Everything the views bind to is on the main actor; snapshots are delivered on the main actor by
/// consuming the stream from a main-actor `Task` (step-02 proved this works).
@MainActor
@Observable
public final class ProfileViewModel {
    // ---- observable state the views bind to -------------------------------------------------

    /// The latest draft snapshot — the `observe` verb's item; the single source of "what".
    public private(set) var snapshot: ProfileSnapshot
    /// The latest canonical (server) snapshot, for the simulator pane.
    public private(set) var canonical: ProfileSnapshot?
    /// The outcome of the most recent submit, for rendering the report / success.
    public private(set) var lastSubmit: SubmitOutcome?

    /// Per-field local editing buffers — the text the user types into freely (the echo rule). The
    /// core's sanitized value is never written back here while the field is focused.
    public var usernameText: String
    public var nameText: String
    public var emailText: String
    public var startDate: PlainDate
    public var endDate: PlainDate

    /// Measurement: how many uniqueness checks actually fired (a debounce collapses a burst to one).
    public private(set) var checkRunCount = 0

    // ---- private machinery ------------------------------------------------------------------

    private let store: ProfileStoreFfi
    private var draft: ProfileDraftFfi
    private let seed: ProfileValues
    private let makeChecker: () -> any UniquenessChecker
    private let debounce: Duration
    private var focused: ProfileFieldId?
    private var draftTask: Task<Void, Never>?
    private var canonicalTask: Task<Void, Never>?
    private var checkTask: Task<Void, Never>?

    /// - Parameters:
    ///   - seed: the initial canonical profile.
    ///   - debounce: how long a valid+dirty username settles before a uniqueness check fires.
    ///   - makeChecker: produces the foreign uniqueness checker set on each checkout (injectable
    ///     for tests / the manual protocol's slow-checker).
    public init(
        seed: ProfileValues,
        debounce: Duration = .milliseconds(400),
        makeChecker: @escaping () -> any UniquenessChecker = { DefaultChecker() }
    ) throws {
        self.store = ProfileStoreFfi()
        try store.applyCanonical(values: seed)
        self.seed = seed
        self.debounce = debounce
        self.makeChecker = makeChecker

        let d = store.checkout()
        d.setUniquenessChecker(checker: makeChecker())
        self.draft = d

        let snap = d.snapshot()
        self.snapshot = snap
        self.canonical = store.canonical()
        self.usernameText = Self.display(snap.username.validity)
        self.nameText = Self.display(snap.name.validity)
        self.emailText = Self.display(snap.email.validity)
        let (start, end) = Self.dateRange(snap.availability.validity, seed: seed)
        self.startDate = start
        self.endDate = end

        // Subscribe FIRST, then reconcile a fresh get() — streams are future-only (step 02), so
        // subscribing before reading closes the get-then-subscribe gap; the `version` stamp dedups.
        subscribeDraft(d)
        subscribeCanonical()
        reconcile(d.snapshot())
    }

    // ---- editing (the echo rule) ------------------------------------------------------------

    public func focus(_ field: ProfileFieldId) { focused = field }

    /// On blur the field is no longer owned by the native control, so its buffer refreshes to the
    /// core's sanitized value (or the retained `Invalid.raw`).
    public func blur(_ field: ProfileFieldId) {
        if focused == field { focused = nil }
        syncBuffers(from: snapshot)
    }

    public func editUsername() {
        guard draft.isLive() else { return }
        try? draft.trySetUsername(raw: usernameText) // per-keystroke try_set — the bet, exercised
        reconcile(draft.snapshot())                  // self-update; the focused buffer is untouched
        scheduleCheck()
    }

    public func editName() {
        guard draft.isLive() else { return }
        try? draft.trySetName(raw: nameText)
        reconcile(draft.snapshot())
    }

    public func editEmail() {
        guard draft.isLive() else { return }
        try? draft.trySetEmail(raw: emailText)
        reconcile(draft.snapshot())
    }

    public func editAvailability() {
        guard draft.isLive() else { return }
        try? draft.trySetAvailability(start: startDate, end: endDate)
        reconcile(draft.snapshot())
    }

    // ---- async uniqueness check -------------------------------------------------------------

    /// Debounced trigger: only a valid AND dirty username is worth checking. Each edit cancels the
    /// pending timer (single-flight collapses a burst); because a value change resets the check in
    /// the core (invariant 13), a keystroke during a pending check invalidates its verdict for free.
    private func scheduleCheck() {
        checkTask?.cancel()
        guard case .valid = snapshot.username.validity, snapshot.username.dirty else { return }
        let interval = debounce
        checkTask = Task { [weak self] in
            try? await Task.sleep(for: interval)
            guard !Task.isCancelled else { return }
            self?.runCheckNow()
        }
    }

    /// Drive one uniqueness check off the main actor (the foreign checker may block); its Pending
    /// and verdict snapshots arrive back on the draft stream. Exposed for tests (bypasses debounce).
    public func runCheckNow() {
        guard draft.isLive() else { return }
        checkRunCount += 1
        let driver = CheckDriver(draft)
        Task.detached { driver.run() }
    }

    /// `true` while a uniqueness check is in flight — the spinner binds to this.
    public var isChecking: Bool { snapshot.usernameCheck == .pending }

    // ---- conflict resolution ----------------------------------------------------------------

    public func resolveKeepMine(_ field: ProfileFieldId) {
        guard draft.isLive() else { return }
        draft.resolveKeepMine(field: field)
        applyResolved(field)
    }

    public func resolveTakeTheirs(_ field: ProfileFieldId) {
        guard draft.isLive() else { return }
        draft.resolveTakeTheirs(field: field)
        applyResolved(field)
    }

    /// A resolution moves the field's value from outside a keystroke, so its buffer refreshes even
    /// if focused (unlike per-keystroke sanitization).
    private func applyResolved(_ field: ProfileFieldId) {
        let snap = draft.snapshot()
        snapshot = snap
        syncBuffers(from: snap, force: field)
    }

    // ---- submit -----------------------------------------------------------------------------

    public func submit() {
        guard draft.isLive() else { lastSubmit = .alreadySubmitted; return }
        do {
            try draft.submit()
            lastSubmit = .success
            recheckout() // the draft tombstoned on success; start a fresh edit session
        } catch let error as SubmitErrorFfi {
            lastSubmit = Self.outcome(of: error) // draft still alive (F3) — keep editing
        } catch {
            lastSubmit = nil
        }
    }

    // ---- server simulator (stands in for a backend) -----------------------------------------

    public enum ServerChange: Equatable, Sendable {
        case username(String)
        case name(String)
        case email(String)
        case resetToSeed
    }

    /// Apply a canonical change, the live-rebase / conflict driver. The draft rebases underneath and
    /// its stream delivers the result (clean fields adopt; dirty fields conflict).
    public func applyServerChange(_ change: ServerChange) {
        guard var values = currentCanonicalValues() else { return }
        switch change {
        case .username(let u): values.username = u
        case .name(let n): values.name = n
        case .email(let e): values.email = e
        case .resetToSeed: values = seed
        }
        try? store.applyCanonical(values: values)
    }

    // ---- constraint-derived UI affordances (NO literals here) -------------------------------

    public func constraints(_ field: ProfileFieldId) -> [ConstraintFfi] {
        store.constraints(field: field)
    }

    /// Max character count for a field, from `LenChars` metadata — `nil` if the field has none.
    public func maxLength(_ field: ProfileFieldId) -> Int? {
        for constraint in constraints(field) {
            if case .lenChars(_, let max) = constraint { return Int(max) }
        }
        return nil
    }

    public func isRequired(_ field: ProfileFieldId) -> Bool {
        constraints(field).contains(.required)
    }

    // ---- error rendering --------------------------------------------------------------------

    /// The inline error for a field: its tier-1 `Invalid` error, plus (for username) a failed
    /// uniqueness verdict. The full report (required, rules) surfaces on submit via `lastSubmit`.
    public func inlineError(_ field: ProfileFieldId) -> String? {
        if let validityError = Self.validityError(field, in: snapshot) {
            return Localization.message(validityError)
        }
        if field == .username, case .failed(let error) = snapshot.usernameCheck {
            return Localization.message(error)
        }
        return nil
    }

    public func isDirty(_ field: ProfileFieldId) -> Bool {
        switch field {
        case .username: snapshot.username.dirty
        case .name: snapshot.name.dirty
        case .email: snapshot.email.dirty
        case .availability: snapshot.availability.dirty
        }
    }

    /// Conflict banner data for a field, if conflicted: the incoming `theirs` (and `base`) as text.
    public func conflict(_ field: ProfileFieldId) -> ConflictInfo? {
        switch field {
        case .username:
            if case .conflicted(let base, let theirs) = snapshot.username.sync {
                return ConflictInfo(base: base, theirs: theirs)
            }
        case .name:
            if case .conflicted(let base, let theirs) = snapshot.name.sync {
                return ConflictInfo(base: base, theirs: theirs)
            }
        case .email:
            if case .conflicted(let base, let theirs) = snapshot.email.sync {
                return ConflictInfo(base: base, theirs: theirs)
            }
        case .availability:
            if case .conflicted(let base, let theirs) = snapshot.availability.sync {
                return ConflictInfo(
                    base: base.map(Self.rangeText), theirs: Self.rangeText(theirs)
                )
            }
        }
        return nil
    }

    // ---- private: streams + reconcile -------------------------------------------------------

    private func subscribeDraft(_ d: ProfileDraftFfi) {
        let stream = d.snapshots()
        draftTask?.cancel()
        draftTask = Task { [weak self] in
            for await snap in stream { self?.reconcile(snap) }
        }
    }

    private func subscribeCanonical() {
        let stream = store.snapshots()
        canonicalTask?.cancel()
        canonicalTask = Task { [weak self] in
            for await snap in stream { self?.canonical = snap }
        }
    }

    /// Version-guarded reconcile. A snapshot with an OLDER `base_version` is a stale rebase and is
    /// dropped (the subscribe-race guard); an equal version is an edit/check update and is taken
    /// (the stream is FIFO per subscriber). Buffers refresh for every field except the focused one.
    private func reconcile(_ snap: ProfileSnapshot) {
        if snap.version < snapshot.version { return }
        snapshot = snap
        syncBuffers(from: snap)
    }

    private func recheckout() {
        let d = store.checkout()
        d.setUniquenessChecker(checker: makeChecker())
        draft = d
        focused = nil
        subscribeDraft(d)
        let snap = d.snapshot()
        snapshot = snap
        syncBuffers(from: snap)
    }

    /// Refresh editing buffers from a snapshot, skipping the focused field (echo rule) unless
    /// `force` names it (a value moved from outside a keystroke, e.g. a resolution).
    private func syncBuffers(from snap: ProfileSnapshot, force: ProfileFieldId? = nil) {
        setBuffer(.username, Self.display(snap.username.validity), force: force)
        setBuffer(.name, Self.display(snap.name.validity), force: force)
        setBuffer(.email, Self.display(snap.email.validity), force: force)
        if force == .availability || focused != .availability {
            let (start, end) = Self.dateRange(snap.availability.validity, seed: seed)
            startDate = start
            endDate = end
        }
    }

    private func setBuffer(_ field: ProfileFieldId, _ value: String, force: ProfileFieldId?) {
        if focused == field && force != field { return }
        switch field {
        case .username: usernameText = value
        case .name: nameText = value
        case .email: emailText = value
        case .availability: break
        }
    }

    private func currentCanonicalValues() -> ProfileValues? {
        guard let c = canonical else { return nil }
        guard case .valid(let u) = c.username.validity,
              case .valid(let n) = c.name.validity,
              case .valid(let e) = c.email.validity,
              case .valid(let a) = c.availability.validity
        else { return seed }
        return ProfileValues(username: u, name: n, email: e, availability: a)
    }
}

// ---- outcome / helper types -------------------------------------------------------------------

public enum SubmitOutcome: Equatable, Sendable {
    case success
    case validation(ValidationReportFfi)
    case conflicted([ProfileFieldId])
    case orphaned
    case alreadySubmitted
}

public struct ConflictInfo: Equatable, Sendable {
    public let base: String?
    public let theirs: String
}

/// Wraps a (non-`Sendable`) draft handle so a blocking uniqueness check can run on a background
/// queue. Safe: `ProfileDraftFfi` is internally synchronised (an `Arc<Mutex<…>>` on the Rust side).
final class CheckDriver: @unchecked Sendable {
    private let draft: ProfileDraftFfi
    init(_ draft: ProfileDraftFfi) { self.draft = draft }
    func run() { _ = draft.runUsernameCheck() }
}

/// A default foreign uniqueness checker: a small in-memory taken-set, so the manual tester can see
/// a `.failed` verdict without a backend.
public final class DefaultChecker: UniquenessChecker, @unchecked Sendable {
    private let taken: Set<String>
    private let delay: Duration
    public init(taken: Set<String> = ["taken", "admin", "root"], delay: Duration = .zero) {
        self.taken = taken
        self.delay = delay
    }
    public func checkUnique(username: String) -> UniquenessVerdictFfi {
        if delay != .zero { Thread.sleep(forTimeInterval: Double(delay.components.seconds)) }
        return taken.contains(username.lowercased()) ? .taken : .unique
    }
}

// ---- static projection helpers (the monomorphic per-value cost, now on the Swift side) --------

extension ProfileViewModel {
    static func outcome(of error: SubmitErrorFfi) -> SubmitOutcome {
        switch error {
        case .validation(let report): .validation(report)
        case .conflicted(let fields): .conflicted(fields)
        case .orphaned: .orphaned
        case .alreadySubmitted: .alreadySubmitted
        }
    }

    static func display(_ v: UsernameValidity) -> String {
        switch v {
        case .unset: ""
        case .valid(let value): value
        case .invalid(let raw, _): raw
        }
    }

    static func display(_ v: PersonNameValidity) -> String {
        switch v {
        case .unset: ""
        case .valid(let value): value
        case .invalid(let raw, _): raw
        }
    }

    static func display(_ v: EmailValidity) -> String {
        switch v {
        case .unset: ""
        case .valid(let value): value
        case .invalid(let raw, _): raw
        }
    }

    static func dateRange(_ v: AvailabilityValidity, seed: ProfileValues) -> (PlainDate, PlainDate) {
        switch v {
        case .valid(let range): (range.start, range.end)
        case .invalid(let raw, _): (raw.start, raw.end)
        case .unset: (seed.availability.start, seed.availability.end)
        }
    }

    static func rangeText(_ r: PlainDateRange) -> String {
        "\(dateText(r.start)) → \(dateText(r.end))"
    }

    static func dateText(_ d: PlainDate) -> String {
        String(format: "%04d-%02d-%02d", Int(d.year), Int(d.month), Int(d.day))
    }

    static func validityError(_ field: ProfileFieldId, in snap: ProfileSnapshot) -> ErrorData? {
        switch field {
        case .username: if case .invalid(_, let e) = snap.username.validity { return e }
        case .name: if case .invalid(_, let e) = snap.name.validity { return e }
        case .email: if case .invalid(_, let e) = snap.email.validity { return e }
        case .availability: if case .invalid(_, let e) = snap.availability.validity { return e }
        }
        return nil
    }
}
