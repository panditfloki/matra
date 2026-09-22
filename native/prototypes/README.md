# Rejected native glass experiment

`rejected-clipped-glass.rs` is not part of the Cargo module graph. Do not wire it
back in without addressing the compositor failure seen on this Windows machine.

Native Blur and Acrylic both returned success but drew an unwanted backdrop over
the transparent overlay rectangle, outside the visible rounded pill. Region
clipping did not solve it. Controlled stripe screenshots and a blur-disabled
negative control caught the failure. Preview.6 was never installed or published.

The shipping preview.5 Glass mode is light translucency only. A promising next
experiment is separate small native notch/backdrop and hover-card windows, not
applying native effects to the current oversized transparent WebView window.
This is unproven and must be visually tested against desktop-composited pixels.
