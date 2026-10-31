# Mac 1.9.0 Beta 2: launch correction

Build **1902** corrects the launch failure in the Mac Beta 1 package (1901).
Quit the earlier app, then install the new DMG from the
[Beta 2 release](https://github.com/panditfloki/matra/releases/tag/v1.9.0-beta.2).
Windows Beta 1 is unchanged.

The app was signed ad-hoc with hardened runtime enabled. The dynamic loader's
library validation rejected Sparkle because ad-hoc signatures have no Apple Team
ID. Static `codesign --verify --deep --strict` passed, but the app aborted before
its own code ran. The old package reproduced this failure outside a debugger.

This packaging-only correction uses the same compiled feature code, increments
the bundle build to 1902, and signs the test app without hardened runtime or
debugger entitlements. It does not add a library-validation exception or change
Gatekeeper, SIP, the user's settings or accounts. It remains ad-hoc signed and
not notarized. A future Developer ID distribution must explicitly enable hardened
runtime, sign nested code with the same identity, and complete notarization.

The packaging configuration now matches that signing mode. The local packager
strips debug paths and checks the actual signed executable outside a debugger.
`scripts/verify-package-launch.py APP OUTPUT [--arch arm64|x86_64]` uses the app's
existing XCTest startup guard to stop provider initialization and older-instance
retirement. An OS sandbox denies network and writes outside the evidence folder.
It fails if the app exits early or the Sparkle framework is not mapped.

These launch checks establish loader compatibility. They do not verify first-open
Gatekeeper acceptance, live accounts or Sparkle update installation. These beta
packages still require manual installation; no stable update feed is changed.

References: [Sparkle setup](https://sparkle-project.org/documentation/),
[Sparkle code signing](https://sparkle-project.org/documentation/sandboxing/#code-signing),
[Apple hardened runtime](https://help.apple.com/xcode/mac/current/en.lproj/devf87a2ac8f.html).
