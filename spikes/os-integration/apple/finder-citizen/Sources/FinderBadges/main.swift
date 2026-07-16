// The FinderSync extension executable. An .appex has no `main` of its own in Xcode builds —
// the template links the extension entry point. Hand-assembled (recon R1), we call it
// explicitly: NSExtensionMain never returns; it hosts the NSExtension listener and
// instantiates NSExtensionPrincipalClass (FinderSyncHandler) from the appex Info.plist.
//
// M0 skeleton: entry-point ceremony only. The probe behavior (connect + ping from the
// OS-spawned process) is M1; badges and the context-menu command are M4.

import Foundation

@_silgen_name("NSExtensionMain")
func NSExtensionMain() -> Never

NSExtensionMain()
