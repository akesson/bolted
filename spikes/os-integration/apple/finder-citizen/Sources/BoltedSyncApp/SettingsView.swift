// The settings editor — step 03's form, remote edition. Everything rendered here is returned
// data: field raws, keyed errors (ErrorMessages templates + core params), conflict triples,
// submit refusals. The only shell-owned numbers are timing (the debounce) — *when*, never *what*.

import BoltedSyncCore
import SwiftUI
import SyncWireKit

struct SettingsView: View {
    @Bindable var vm: SyncViewModel
    @FocusState private var focus: String?

    /// Debounce for the folder check — a shell-taste constant (when, not what).
    private static let checkDebounce: Duration = .milliseconds(300)

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            if vm.draft == nil {
                ContentUnavailableView(
                    "No draft open",
                    systemImage: "square.and.pencil",
                    description: Text(vm.connectionState == .connected
                        ? "Open the editor from the menu bar."
                        : "The daemon is not connected."))
            } else {
                if vm.restoredFromStash {
                    Label("Restored your unsaved edits from before the interruption.",
                          systemImage: "arrow.uturn.backward.circle")
                        .foregroundStyle(.orange)
                }
                field("label", "Label", $vm.labelBuffer)
                field("folder", "Folder", $vm.folderBuffer)
                    .task(id: vm.folderBuffer) {
                        try? await Task.sleep(for: Self.checkDebounce)
                        guard !Task.isCancelled else { return }
                        vm.runFolderCheckIfNeeded()
                    }
                field("interval", "Interval (minutes)", $vm.intervalBuffer)
                pausedRow
                submitRow
            }
        }
        .padding(20)
        .frame(minWidth: 420)
        .onChange(of: focus) { old, _ in
            if let old { vm.blur(field: old) }
            vm.focusedField = focus
        }
    }

    @ViewBuilder
    private func field(_ name: String, _ title: String, _ buffer: Binding<String>) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(title)
                if vm.draft?.field(name)?.dirty == true {
                    Circle().fill(.blue).frame(width: 6, height: 6)
                }
                if name == "folder", pendingCheck {
                    ProgressView().controlSize(.small)
                }
            }
            TextField(title, text: buffer)
                .textFieldStyle(.roundedBorder)
                .focused($focus, equals: name)
                .onChange(of: buffer.wrappedValue) { _, text in
                    // Per-keystroke try_set: the buffer already holds the user's text; the
                    // core judges, the focused buffer is never rewritten from core (§6).
                    if focus == name { vm.edit(field: name, text: text) }
                }
            ForEach(errors(for: name), id: \.key) { err in
                Text(ErrorMessages.render(err))
                    .font(.caption)
                    .foregroundStyle(err.key == "folder_check_pending" ? .secondary : Color.red)
            }
            if let f = vm.draft?.field(name), let theirs = f.theirs {
                conflictBanner(name: name, field: f, theirs: theirs)
            }
        }
    }

    @ViewBuilder
    private func conflictBanner(name: String, field: FieldW, theirs: RawW) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Label("Changed elsewhere while you were editing", systemImage: "exclamationmark.triangle")
                .font(.caption)
            HStack(spacing: 12) {
                Text("Mine: \(field.raw?.asText ?? "—")").font(.caption)
                Text("Theirs: \(theirs.asText ?? "—")").font(.caption)
            }
            HStack {
                Button("Keep mine") { vm.resolve(field: name, keepMine: true) }
                Button("Take theirs") { vm.resolve(field: name, keepMine: false) }
            }
            .controlSize(.small)
        }
        .padding(8)
        .background(.yellow.opacity(0.15), in: RoundedRectangle(cornerRadius: 6))
    }

    private var pausedRow: some View {
        Toggle(
            "Paused",
            isOn: Binding(
                get: { vm.draft?.paused.raw == .flag(true) },
                set: { vm.setPaused($0) }
            ))
    }

    @ViewBuilder
    private var submitRow: some View {
        HStack {
            Button("Save") { vm.submit() }
                .keyboardShortcut(.defaultAction)
            Button("Discard") { vm.closeEditor() }
            Spacer()
            switch vm.lastSubmit {
            case .submitted(let version):
                Text("Saved (v\(version))").foregroundStyle(.green)
            case .refusedValidation:
                Text("Fix the errors above to save.").foregroundStyle(.red)
            case .refusedConflicted(let fields):
                Text("Resolve conflicts first: \(fields.joined(separator: ", "))")
                    .foregroundStyle(.red)
            case .refusedOrphaned:
                Text("This draft no longer has a base — discard it.").foregroundStyle(.red)
            case nil:
                EmptyView()
            }
        }
    }

    private var pendingCheck: Bool {
        vm.draft?.report.ruleKeys.contains("folder_check_pending") == true
    }

    private func errors(for field: String) -> [ErrorW] {
        vm.draft?.report.errors(for: field) ?? []
    }
}
