# Share folder

`Matra Notch/Matra-Setup.exe` is the current local Windows installer. This folder
is ignored by Git and contains no source tree, developer tools or credentials.
`VERSION.json` identifies its exact version, checksum and release/test status.

`npm run native:bundle` builds the setup and calls
`scripts/prepare-share-folder.ps1`. When the binary differs from the current one,
the old setup and its metadata/notices move into a timestamped `Archive/` child.
An identical rebuild does not create another archive entry. Nothing is pruned.

Share the current files with their license notices, not the entire repository or
the Archive directory. Setup installs the application; it is not a portable app.
Launch the installed app from Start, not by repeatedly launching Setup.

Uninstalling for a user-journey test is a separate explicit action. Matra and
upstream CodeNotch have separate installations. Keep personal settings unless a
full preferences reset is specifically requested. Removing developer source files
does not uninstall an app.

Branding and original Matra customisations are separate from licensed upstream
code. Required MIT and provider notices remain; packaging is not an ownership
transfer. Future installers embed the notices as well as shipping them alongside.
