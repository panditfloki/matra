# Provider marks

The SVG files in this directory come from the npm package `@lobehub/icons-static-svg` 1.95.0
(https://github.com/lobehub/lobe-icons, MIT License) and are unmodified:

| File | Original file in the package | Shown in |
|---|---|---|
| claude.svg | icons/claude.svg | Claude cell |
| codex.svg | icons/openai.svg | Codex cell (the OpenAI mark, matching upstream Matra's glyph choice) |
| codex-alt.svg | icons/codex.svg | alternative: Codex's own mark |
| cursor.svg | icons/cursor.svg | Cursor cell |
| grok.svg | icons/grok.svg | Grok cell |
| gemini.svg | icons/antigravity.svg | Antigravity cell |
| gemini-alt.svg | icons/gemini.svg | alternative: the Gemini spark |

MIT License — Copyright (c) LobeHub. See that repository's LICENSE.

**Trademarks**: these marks are trademarks of Anthropic, OpenAI, Anysphere (Cursor), xAI (Grok) and Google
respectively, and are used here only to identify the product whose usage is displayed. Whether
they stay in a distributed build is the repository owner's call under each brand's guidelines;
they can be swapped for generated glyphs without touching any code.

**Overrides**: a file of the same name (`.svg` or `.png`) in `%APPDATA%\matra\glyphs\` takes
precedence over the built-in mark; it is picked up after "Refresh usage now" in the tray menu.

## Shared native Mātrā marks

The Windows build also uses the unmodified Grok Bot, Higgsfield, DeepSeek, Ollama, LM Studio, oMLX and llama.cpp assets already shipped with the Mac source. These identify the corresponding providers. See the bundled third-party source license. The llama.cpp mark comes from ggml-org/llama.brand; its attribution and express software-distribution permission are preserved in LlamaBrand-NOTICE.txt and LlamaBrand-LICENSE.txt.

## Fixed-provider parity marks

MiniMax, Amp, Apify, Kilo and Devin are byte-for-byte copies of the assets in the native Mac source. Gemini API uses the existing Gemini spark (`gemini-alt.svg`), distinct from Antigravity. Kiro is the native `GlyphOutline.kiro` coordinate sequence represented as an SVG path with the same even-odd fill. The native Mac sources and assets are unchanged. Raster Devin is rendered as an alpha mask, as its native template asset specifies. Black SVG fills are mapped to the current text colour only at rendering time.
