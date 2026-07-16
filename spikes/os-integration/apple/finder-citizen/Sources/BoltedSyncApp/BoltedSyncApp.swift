// The menu-bar shell — as thin as step 03's executable target. All behavior lives in
// BoltedSyncCore where it is headless-testable; these files are scene layout only.

import BoltedSyncCore
import SwiftUI

@main
struct BoltedSyncApp: App {
    @State private var vm = SyncViewModel()
    @Environment(\.openWindow) private var openWindow

    var body: some Scene {
        MenuBarExtra("Bolted Sync", systemImage: menuSymbol) {
            MenuContent(vm: vm, openWindow: { openWindow(id: "settings") })
                .onAppear(perform: connectIfNeeded)
        }
        Window("Bolted Sync Settings", id: "settings") {
            SettingsView(vm: vm)
        }
        .windowResizability(.contentSize)
    }

    private var menuSymbol: String {
        vm.canonical?.paused == true
            ? "pause.circle" : "arrow.triangle.2.circlepath.circle"
    }

    private func connectIfNeeded() {
        guard vm.connectionState != .connected else { return }
        guard let group = GroupSocket.groupId(),
            let path = GroupSocket.socketPath(group: group)
        else { return }
        vm.connect(path: path)
    }
}

struct MenuContent: View {
    @Bindable var vm: SyncViewModel
    let openWindow: () -> Void

    var body: some View {
        // U1: everything shown here is the fetched canonical — updated by tick-then-fetch,
        // never by echoing local input.
        switch vm.connectionState {
        case .connected:
            if let c = vm.canonical {
                Text("\(c.label) — every \(c.interval) min")
                Text("Folder: \(c.folder)")
                Text(c.paused ? "Syncing is paused" : "Syncing is active")
                Divider()
                Button(c.paused ? "Resume Syncing" : "Pause Syncing") {
                    vm.togglePaused()
                }
            } else {
                Text("No canonical state")
            }
            Divider()
            Button("Edit Settings…") {
                vm.openEditor()
                openWindow()
            }
        case .disconnected:
            Text("Daemon connection lost")
            Button("Reconnect") { reconnect() }
        case .failed(let why):
            Text("Cannot reach the daemon")
            Text(why).font(.caption)
            Button("Retry") { reconnect() }
        case .idle:
            Text("Not connected")
            Button("Connect") { reconnect() }
        }
        Divider()
        Button("Quit") { NSApp.terminate(nil) }
    }

    private func reconnect() {
        guard let group = GroupSocket.groupId(),
            let path = GroupSocket.socketPath(group: group)
        else { return }
        vm.connect(path: path)
    }
}
