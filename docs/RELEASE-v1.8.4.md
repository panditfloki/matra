# Mātrā v1.8.4

## What's new

- Fixed the side-notch centre-up reveal being invisible when Windows Animation effects are off. The app now plays this reveal by default on hover, independent of that Windows setting.
- Added Appearance → Notch → Expand animation. Turn it off if you prefer the notch to open instantly. Your choice persists across restarts.
- Other activity and ring motion still follows the Windows reduced-motion preference.

## Verification

The rendered UI regression test starts with reduced motion enabled, samples the notch animation midway, and checks that the switch disables and re-enables it. Native tests cover the default for existing configurations and persistence of an explicit choice.

Physical hover in the installed Windows WebView2 window and the two-version install/restart journey still require a live user check. The installer remains unsigned.
