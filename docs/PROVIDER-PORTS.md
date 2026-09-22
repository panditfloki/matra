# Additional Windows providers

Implemented locally in the v1.8.3 candidate; not yet published or live-account
certified. Scope: GitHub Copilot, OpenCode Go, Command Code and Kimi Code.
Ollama, LM Studio, DeepSeek, MiniMax, Qianwen, Gemini API and Kiro remain separate work.

Reference: [CodeNotch providers](https://github.com/vinzdg/codenotch/tree/117a38b8edae2ebd0944bc86b8760c6381685345/Sources/Providers),
MIT licensed. Windows implementation: `native/codenotch/src/extra_providers.rs`.

| ID | Credential source | Reading / headline |
|---|---|---|
| copilot | GH_TOKEN / GITHUB_TOKEN, otherwise installed gh.exe auth token --hostname github.com | api.github.com/copilot_internal/user; premium interactions, else another metered Copilot quota |
| opencode | XDG_DATA_HOME/opencode/auth.json, ~/.local/share/opencode/auth.json, %APPDATA%/opencode/auth.json | opencode.ai/zen/go/v1/usage; rolling 5h, plus weekly/monthly |
| commandcode | COMMAND_CODE_API_KEY, otherwise ~/.commandcode/auth.json apiKey | api.commandcode.ai/alpha whoami, subscriptions, credits and period-scoped summary; monthly spend / (spent + remaining), plus 5h/weekly |
| kimi | KIMI_CODE_HOME or ~/.kimi-code/credentials/kimi-code.json | api.kimi.com/coding/v1/usages; rolling 5h, plus weekly |

Connection instructions are in AI sources. The new readers are off by default.
Switching on starts polling and adds the ring to a custom selection. Switching off
stops future polling and forgets this reader's readings, but never signs out the
tool that owns the credential. An already in-flight request may finish; its result
is discarded using a generation guard. Reads are not persisted across launches.

Only `opencode-go` can authenticate OpenCode Go. Other keys in the same file are
ignored. Kimi tokens without a future expiry are not sent. Token refresh belongs to
Kimi CLI. GitHub enterprise credentials are not borrowed for github.com. The gh
command has a timeout, bounded output, null stdin/stderr and no shell wrapper.

Network reads use the OS trust store, fixed endpoint hosts, no redirects, a 15s
request timeout and a 1MiB JSON limit. No response bodies or secrets are logged.
Polling defaults to five minutes. Manual refresh is throttled and cannot bypass
a provider Retry-After deadline within the current run. Restart does not persist
this reader's backoff yet. Failed/auth-changed readings are cleared, not shown as
fresh or zero. Unlimited/unknown denominators produce no fabricated percentage.

Command Code requires a subscription period start so lifetime cost is not mixed
with this month's credit balance. This is deliberately stricter than upstream.
Known windows have explicit headline selectors in Rust and JavaScript; a missing
Kimi/OpenCode rolling window never promotes weekly into its place.

Tests cover credential isolation, expired/missing tokens, string counts, missing
denominators, unlimited Copilot quotas, current-period arithmetic, URL encoding,
UI enable/disable and ring selectors. Fixtures do not log in or call paid inference.
Live provider accounts and vendor endpoint compatibility still require validation.
Some endpoints are the vendors' internal tool APIs, not stable public contracts.

New providers show quota changes, not per-turn completion animations. No completion
signal is invented for tools without an activity adapter. A long notch can scroll
within its viewport; labels use readable letter glyphs until branded provider icons
are reviewed. Original six provider identities are unchanged.
