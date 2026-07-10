import SwiftUI
import AppKit
import GenProfileFfi
import ProfileFeature

/// Promotes this CLI-launched, *unbundled* executable to a regular foreground app. A bare SwiftPM
/// binary has no `Info.plist`, so LaunchServices registers it `BackgroundOnly` — the run loop runs
/// but `WindowGroup` surfaces no focusable window (it just looks "stuck"). Setting the activation
/// policy and activating on launch gives us a real window, Dock icon, and menu bar. Only the manual
/// protocol runs this; `test:apple` never does.
final class AppActivator: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
    }
}

/// The thin `@main` shell: seeds a profile, builds the ViewModel, shows the editor. Run with
/// `mise run run:apple`. The uniqueness checker carries a 1 s delay so the spinner is visible
/// during the manual protocol.
@main
struct ProfileApp: App {
    @NSApplicationDelegateAdaptor(AppActivator.self) private var activator
    @State private var model: ProfileViewModel?

    init() {
        let seed = ProfileValues(
            username: "alice",
            name: "Alice Smith",
            email: "alice@example.com",
            availability: AvailabilityRaw(
                start: PlainDate(year: 2026, month: 1, day: 1),
                end: PlainDate(year: 2026, month: 12, day: 31)
            )
        )
        let vm = try? ProfileViewModel(
            seed: seed,
            debounce: .milliseconds(400),
            makeChecker: { DefaultChecker(delay: .seconds(1)) }
        )
        _model = State(initialValue: vm)
    }

    var body: some Scene {
        WindowGroup("Bolted — Profile Spike") {
            if let model {
                ProfileFormView(vm: model)
            } else {
                Text("Failed to initialise the profile store.")
                    .padding()
            }
        }
        .windowResizability(.contentSize)
    }
}
