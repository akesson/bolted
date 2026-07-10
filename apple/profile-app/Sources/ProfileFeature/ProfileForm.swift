import SwiftUI
import GenProfileFfi

/// The profile editor. Every "what" it shows — labels' required markers, counters' maxima, error
/// sentences' numbers, conflict data — comes from the core via the ViewModel; the view adds only
/// layout and *when* (focus, taps). No constraint or rule value is written here.
public struct ProfileFormView: View {
    @Bindable var vm: ProfileViewModel
    @FocusState private var focusedField: ProfileFieldId?

    public init(vm: ProfileViewModel) { self.vm = vm }

    public var body: some View {
        HStack(alignment: .top, spacing: 0) {
            editor
                .frame(minWidth: 360)
                .padding()
            Divider()
            ServerSimulatorPane(vm: vm)
                .frame(width: 260)
                .padding()
        }
        .frame(minWidth: 660, minHeight: 560)
        .onChange(of: focusedField) { old, new in
            if let old { vm.blur(old) }
            if let new { vm.focus(new) }
        }
    }

    private var editor: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text("Edit profile").font(.title2).bold()

                TextFieldRow(
                    field: .username, vm: vm, text: $vm.usernameText,
                    focus: $focusedField, onEdit: vm.editUsername, showsSpinner: true
                )
                TextFieldRow(
                    field: .name, vm: vm, text: $vm.nameText,
                    focus: $focusedField, onEdit: vm.editName, showsSpinner: false
                )
                TextFieldRow(
                    field: .email, vm: vm, text: $vm.emailText,
                    focus: $focusedField, onEdit: vm.editEmail, showsSpinner: false
                )
                AvailabilityRow(vm: vm)

                HStack {
                    Button("Submit") { vm.submit() }
                        .keyboardShortcut(.return, modifiers: .command)
                        .accessibilityIdentifier("submit")
                    if vm.snapshot.anyDirty {
                        Text("unsaved changes").font(.caption).foregroundStyle(.secondary)
                    }
                }
                if let outcome = vm.lastSubmit {
                    SubmitResultView(outcome: outcome)
                }
            }
        }
    }
}

/// One text field with its constraint-derived counter/required marker, dirty dot, spinner, inline
/// error and conflict banner. The `text` binding's setter triggers `onEdit` on user input only —
/// a programmatic buffer refresh (blur / rebase) updates the value without re-firing the edit.
struct TextFieldRow: View {
    let field: ProfileFieldId
    let vm: ProfileViewModel
    @Binding var text: String
    var focus: FocusState<ProfileFieldId?>.Binding
    let onEdit: () -> Void
    let showsSpinner: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 6) {
                Text(field.label + (vm.isRequired(field) ? " *" : ""))
                    .font(.caption).foregroundStyle(.secondary)
                if vm.isDirty(field) {
                    Circle().fill(.orange).frame(width: 6, height: 6)
                }
                Spacer()
                if showsSpinner && vm.isChecking {
                    ProgressView().controlSize(.small)
                        .accessibilityIdentifier("spinner-\(field.idString)")
                }
                if let max = vm.maxLength(field) {
                    Text("\(text.count)/\(max)")
                        .font(.caption2)
                        .foregroundStyle(text.count > max ? .red : .secondary)
                        .monospacedDigit()
                }
            }
            TextField(field.label, text: Binding(get: { text }, set: { text = $0; onEdit() }))
                .textFieldStyle(.roundedBorder)
                .focused(focus, equals: field)
                .accessibilityIdentifier("field-\(field.idString)")
            if let error = vm.inlineError(field) {
                Text(error).font(.caption).foregroundStyle(.red)
                    .accessibilityIdentifier("error-\(field.idString)")
            }
            if let info = vm.conflict(field) {
                ConflictBanner(field: field, info: info, vm: vm)
            }
        }
    }
}

struct AvailabilityRow: View {
    let vm: ProfileViewModel

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 6) {
                Text("Availability" + (vm.isRequired(.availability) ? " *" : ""))
                    .font(.caption).foregroundStyle(.secondary)
                if vm.isDirty(.availability) {
                    Circle().fill(.orange).frame(width: 6, height: 6)
                }
            }
            DatePicker(
                "Start",
                selection: Binding(
                    get: { Self.date(vm.startDate) },
                    set: { vm.startDate = Self.plain($0); vm.editAvailability() }
                ),
                displayedComponents: .date
            )
            DatePicker(
                "End",
                selection: Binding(
                    get: { Self.date(vm.endDate) },
                    set: { vm.endDate = Self.plain($0); vm.editAvailability() }
                ),
                displayedComponents: .date
            )
            if let error = vm.inlineError(.availability) {
                Text(error).font(.caption).foregroundStyle(.red)
            }
            if let info = vm.conflict(.availability) {
                ConflictBanner(field: .availability, info: info, vm: vm)
            }
        }
    }

    static func date(_ p: PlainDate) -> Date {
        var c = DateComponents()
        c.year = Int(p.year); c.month = Int(p.month); c.day = Int(p.day)
        return Calendar.current.date(from: c) ?? Date()
    }

    static func plain(_ d: Date) -> PlainDate {
        let c = Calendar.current.dateComponents([.year, .month, .day], from: d)
        return PlainDate(
            year: UInt16(c.year ?? 2026), month: UInt8(c.month ?? 1), day: UInt8(c.day ?? 1)
        )
    }
}

struct ConflictBanner: View {
    let field: ProfileFieldId
    let info: ConflictInfo
    let vm: ProfileViewModel

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("Server changed this field").font(.caption).bold()
            HStack(spacing: 6) {
                Text("theirs:").font(.caption2).foregroundStyle(.secondary)
                Text(info.theirs).font(.caption)
                    .accessibilityIdentifier("conflict-theirs-\(field.idString)")
                if let base = info.base {
                    Text("(was \(base))").font(.caption2).foregroundStyle(.secondary)
                }
            }
            HStack {
                Button("Keep mine") { vm.resolveKeepMine(field) }
                    .accessibilityIdentifier("keepmine-\(field.idString)")
                Button("Take theirs") { vm.resolveTakeTheirs(field) }
                    .accessibilityIdentifier("taketheirs-\(field.idString)")
            }
            .controlSize(.small)
        }
        .padding(8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.yellow.opacity(0.15))
        .clipShape(RoundedRectangle(cornerRadius: 6))
        // NOTE: no container-level accessibilityIdentifier here — on this VStack it propagates to and
        // clobbers the children's ids (theirs text, keep-mine/take-theirs buttons). Detect the banner
        // via the per-element `conflict-theirs-<field>` instead.
    }
}

struct SubmitResultView: View {
    let outcome: SubmitOutcome

    var body: some View {
        switch outcome {
        case .success:
            Label("Submitted", systemImage: "checkmark.circle.fill")
                .font(.caption).foregroundStyle(.green)
                .accessibilityIdentifier("submit-success")
        case .validation(let report):
            VStack(alignment: .leading, spacing: 2) {
                Text("Fix these before submitting:").font(.caption).bold()
                ForEach(Array(report.fieldErrors.enumerated()), id: \.offset) { _, fe in
                    Text("• \(fe.field.label): \(Localization.message(fe.error))").font(.caption)
                }
                ForEach(Array(report.ruleErrors.enumerated()), id: \.offset) { _, re in
                    Text("• \(Localization.message(re.error))").font(.caption)
                }
            }
            .foregroundStyle(.red)
            .accessibilityIdentifier("submit-validation")
        case .conflicted(let fields):
            Text("Resolve conflicts: \(fields.map(\.label).joined(separator: ", "))")
                .font(.caption).foregroundStyle(.orange)
                .accessibilityIdentifier("submit-conflicted")
        case .orphaned:
            Text("This profile was deleted on the server.").font(.caption).foregroundStyle(.red)
                .accessibilityIdentifier("submit-orphaned")
        case .alreadySubmitted:
            Text("Already submitted.").font(.caption).foregroundStyle(.secondary)
                .accessibilityIdentifier("submit-alreadySubmitted")
        }
    }
}

/// Stands in for a backend: shows canonical state and drives `applyCanonical` presets — the
/// live-rebase / conflict source. Editing a field then triggering the matching preset produces a
/// conflict; leaving it clean produces a silent adopt.
struct ServerSimulatorPane: View {
    let vm: ProfileViewModel

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Server simulator").font(.headline)
            if let c = vm.canonical {
                VStack(alignment: .leading, spacing: 2) {
                    Text("canonical").font(.caption2).foregroundStyle(.secondary)
                    Text("username: \(valueText(c.username.validity))").font(.caption)
                        .accessibilityIdentifier("canonical-username")
                    Text("name: \(valueText(c.name.validity))").font(.caption)
                        .accessibilityIdentifier("canonical-name")
                    Text("email: \(valueText(c.email.validity))").font(.caption)
                        .accessibilityIdentifier("canonical-email")
                }
            }
            Divider()
            Text("push a canonical change").font(.caption2).foregroundStyle(.secondary)
            Button("username → server_user") { vm.applyServerChange(.username("server_user")) }
                .accessibilityIdentifier("sim-username")
            Button("name → Server Name") { vm.applyServerChange(.name("Server Name")) }
                .accessibilityIdentifier("sim-name")
            Button("email → team@corp.example") { vm.applyServerChange(.email("team@corp.example")) }
                .accessibilityIdentifier("sim-email")
            Button("reset to seed") { vm.applyServerChange(.resetToSeed) }
                .accessibilityIdentifier("sim-reset")
            Spacer()
        }
        .frame(maxHeight: .infinity, alignment: .top)
    }

    // D24: one shared TextValidity for the three text fields, so one helper.
    func valueText(_ v: TextValidity) -> String {
        if case .valid(let s) = v { return s }
        return "—"
    }
}

extension ProfileFieldId {
    var label: String {
        switch self {
        case .username: "Username"
        case .name: "Name"
        case .email: "Email"
        case .availability: "Availability"
        }
    }

    /// Stable, non-localized token for `.accessibilityIdentifier` so XCUITest (apple/profile-app
    /// UITests) can address fields without depending on display labels.
    var idString: String {
        switch self {
        case .username: "username"
        case .name: "name"
        case .email: "email"
        case .availability: "availability"
        }
    }
}
