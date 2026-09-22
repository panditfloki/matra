/* English product copy; original keys keep the upstream translation catalog usable.
   Brand replacement also applies to translated labels, never to provider identities. */
(() => {
  const copy = {
    'Accounts': 'AI sources',
    'Appearance': 'Display',
    'General': 'App & data',
    'Connected': 'On your meter',
    'Not connected': 'Available sources',
    'Notch': 'Edge meter',
    'Show': 'Visibility',
    'Always show': 'Always visible',
    'Show on hover': 'On hover',
    'Hide': 'Hidden',
    'Size': 'Scale',
    'Weekly ring': 'Weekly measure',
    'Edge': 'Screen edge',
    'App': 'Desktop',
    'App icon': 'Tray icon',
    'Neither': 'Hidden',
    'Show move handle': 'Enable drag handle',
    'Recentre': 'Centre meter',
    'Open Codenotch at login': 'Start Mātrā at login',
    'Let Claude Code notify Codenotch': 'Show Claude session activity',
    'Notch reads': 'Limit window',
    'Model data': 'Model group',
    'One provider always stays ticked, so the pill is never empty.': 'Keep at least one source on your meter. Each source keeps its own limits.',
    'These have no ring to place. Switch one on and it joins the end of the list above.': 'Enable a source to add its readings to the meter.',
    'Turning this on adds a few lines to your Claude Code settings file so that Claude Code sends Codenotch a short message when a session starts, when it is working, when it is waiting for your answer, and when it finishes. That is what makes the ring spin and the amber ring pulse.': 'Adds Mātrā hooks to Claude Code settings. The meter can then show when Claude is working, waiting for you, or finished.',
    'The notch stays open with every reading visible.': 'Keep your usage readings visible at the screen edge.',
    'A small pill at the screen edge that opens when you reach it.': 'Move to the slim edge marker to reveal your readings.',
    'The black pill against the right-hand edge of the screen, with one ring per tool. Turn it off and the pill disappears; Codenotch keeps counting your usage either way.': 'Hide the edge meter. Mātrā continues checking usage in the background.',
    'The small icon down by the clock. Right-clicking it opens this window, refreshes the readings, or quits Codenotch.': 'Right-click the Mātrā icon beside the clock for settings, refresh and quit.',
    'No icon in the system tray. Open Codenotch again from the Start menu to bring these settings back.': 'Tray icon hidden. Open Mātrā from Start to return to settings.',
    'Held on: the notch is hidden, so this icon is the only way left to reach Codenotch. Switch the notch back on first if you want to hide it.': 'The tray icon stays visible while the meter is hidden, so you can always reach Mātrā.',
    'The taskbar icon has been kept on. With the notch hidden it is the only way left to open this window or to quit Codenotch, so Codenotch will not let both of them be switched off at the same time.': 'Tray icon kept visible. Show the edge meter before hiding the tray icon.',
    'Takes the least room on the edge. Readable, but not from across the desk.': 'Compact readings with the smallest desktop footprint.',
    'The size the notch was drawn at.': 'Balanced spacing and readable figures.',
    'Easier to read at a glance, and harder to ignore.': 'Larger rings and figures for easier reading.',
    'One ring per provider, showing the headline limit. The weekly allowance stays in the hover card.': 'Show the primary limit only. Weekly readings remain in the detail card.',
    'A thinner ring for the weekly limit, drawn inside the main one. It shares the gap with the working indicator.': 'Place the weekly measure inside the primary ring. Each ring uses its own limit.',
    'A thinner ring for the weekly limit, drawn around the main one, in the margin between the ring and the notch edge.': 'Place the weekly measure outside the primary ring. Each ring uses its own limit.',
    'Stands upright on the left-hand edge, with the card opening to its right.': 'Anchor left. Details open toward the desktop.',
    'Stands upright on the right-hand edge, the side it was drawn for.': 'Anchor right. Details open toward the desktop.',
    'Lies flat along the top edge, with the card opening below it.': 'Anchor top. Details open below the meter.',
    'Lies flat along the bottom edge, with the card opening above it.': 'Anchor bottom. Details open above the meter.',
    'The arc above the notch. Hold it to carry the notch to another edge — Edge above does the same.': 'Hover near the start of the meter to reveal its drag handle. Hold it to move to another edge.',
    "Hold Alt and drag the notch to slide it along its edge. Each edge remembers where you left it. Recentre puts it back in the middle of the edge it is on, or on the main screen's right-hand edge if the screen it was on has been unplugged.": 'Alt + drag moves the meter along its edge. Each edge remembers its position. Centre meter resets it; if its screen is missing, it returns to the main screen.',
    'Sets the language used by Codenotch. “Follow system” uses the language Windows itself is set to.': 'Choose the interface language, or follow your Windows preference.',
    'Codenotch opens by itself every time you sign in to this computer, and waits quietly in the background until a coding session starts. Turn it off and you have to open Codenotch yourself.': 'Start your meter when you sign in to Windows. Turn this off to launch Mātrā yourself.',
    'Automatic updates are disabled for this Mātrā preview. Install locally reviewed builds from the Mātrā repository.': 'Updates are installed manually. Download the Windows installer from the Mātrā GitHub releases page.',
    'Everything Codenotch remembers lives in one folder: your settings, its log file, and the "glyphs" folder where you can drop your own icons. Opens in File Explorer.': 'Open local preferences, diagnostics and custom provider icons in File Explorer.',
    'Light and Dark are solid. Glass is always light and frosted. System follows Windows.': 'Light and Dark use solid surfaces. Glass uses a translucent smoke tint. System follows Windows.'
  };
  window.matraCopy = (key, fallback, lang = 'en') => {
    const text = lang === 'en' ? (copy[key] || fallback || key) : (fallback || key);
    return String(text).replaceAll('Codenotch', 'Mātrā');
  };
})();
