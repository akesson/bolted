// The FinderSync extension executable. An .appex has no `main` of its own in Xcode builds —
// the template links the extension entry point (`-e _NSExtensionMain`). Hand-assembled
// (recon R1), we call it explicitly from a normal main instead: NSExtensionMain hosts the
// NSExtension listener and instantiates NSExtensionPrincipalClass (FinderSyncHandler) from
// the appex Info.plist. Foundation declares it
// `int NSExtensionMain(int argc, const char *argv[])`; it does not return in practice.

import Foundation

@_silgen_name("NSExtensionMain")
func NSExtensionMain(
    _ argc: Int32,
    _ argv: UnsafePointer<UnsafeMutablePointer<CChar>?>
) -> Int32

exit(NSExtensionMain(CommandLine.argc, CommandLine.unsafeArgv))
