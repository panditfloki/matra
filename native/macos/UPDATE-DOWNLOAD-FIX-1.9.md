# Mac update download correction · build 1904

Builds 1901–1903 used the branded `CFBundleDisplayName` `dy/dx.f(Mātrā)` without
a separate Sparkle host name. Sparkle uses that name and the target build number
as a download directory. The slash made it attempt nested directories with
`withIntermediateDirectories:NO`, so the download failed after receiving the archive.

Build 1904 adds `SUBundleName: Mātrā`. The branded display name remains unchanged.
The packaging launch gate now rejects a host name or build containing a slash
or NUL, before accepting a package for release.

This is a metadata-only correction to the Beta 3 app. Executable, embedded
framework and resource bytes remain unchanged; the build number and Info.plist
change, followed by a fresh non-hardened ad-hoc signature. The earlier Sparkle
launch correction and hover-only drag handle remain included.

## One-time recovery

An affected old app cannot repair this through its updater: the bad host name
comes from the installed bundle before the new app is extracted. Changing a
feed title, archive filename or signature cannot override it. Install Beta 4
manually once, then use Check for updates for future releases. A 1901 copy that
crashes before main also needs manual replacement.

Keep `SUBundleName` safe in every subsequent Mac build. A feed check, standalone
download or signature verification alone does not test Sparkle's download-directory
creation, installation or relaunch.
