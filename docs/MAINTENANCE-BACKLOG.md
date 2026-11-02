# Mātrā maintenance backlog

Maintainer: [Pandit Floki](https://github.com/panditfloki). Updated 2 November 2026.

This is proposed maintenance work with reviewable completion criteria. Existing
tests already cover parts of these behaviours. The items below extend and organise
that coverage; they are not confirmed vulnerabilities or claims of completed work.
No funding, delivery date or external contribution is assumed.

| Item | Priority | Status | Review output |
| --- | --- | --- | --- |
| M-01: Provider parser regressions | High | Planned | Synthetic fixtures, expected outputs and targeted regression results |
| M-02: Account isolation and permission recovery | High | Planned | Account-switch matrix and Mac/Windows regression results |
| M-03: Maintainer automation pilot | After M-01/M-02 fixtures | Planned | Opt-in scripts, evaluation results and usage/cost record |

## Existing evidence

- [Mac Beta 6 release](https://github.com/panditfloki/matra/releases/tag/v1.9.0-beta.6):
  122 focused Mac tests reported passing, with Grok Bot and Cursor fixes.
- [Windows Preview 1.9 workflow](https://github.com/panditfloki/matra/actions/runs/41301477618):
  completed successfully on 1 November 2026 at
  `121746dcf4a0e619fa05ddd9b81f9b2e64f00da1`. The run covers native/UI tests and
  installer lifecycle checks. It does not publish a new Windows stable installer.
- [Mac tests at that source revision](https://github.com/panditfloki/matra/tree/121746dcf4a0e619fa05ddd9b81f9b2e64f00da1/native/macos/Tests)
  and [Windows source/tests](https://github.com/panditfloki/matra/tree/121746dcf4a0e619fa05ddd9b81f9b2e64f00da1/native/matra/src)
  are the starting points for extending coverage.

These are dated results for an existing revision. New changes require their own
checks. Installed updates, real provider permissions and physical-device behaviour
must be recorded separately from fixtures and CI.

## M-01: Provider parser regressions

Goal: keep usage readings correct when provider response formats or account plans
change. Begin with Codex, Claude, Cursor and Grok Bot.

Acceptance criteria:

- [ ] Inventory existing fixtures and add only missing cases: valid zero usage,
  absent session limits, weekly-only limits, multiple/model-specific buckets,
  malformed or partial responses, and stale cached data.
- [ ] Specify the expected window, percentage semantics, account and freshness for
  each fixture. Missing or unparseable readings remain unavailable, never a false zero.
- [ ] Cover reset boundaries and timestamp formats. A session reset must not erase
  a valid weekly reading or make an unrelated model bucket the headline.
- [ ] Use synthetic or redacted data, with no live credentials or private session
  content. A test-only faulty parser must fail at least one targeted regression.
- [ ] Run the relevant Swift and Rust/JavaScript suites. Link the exact commit,
  commands and results in the change; record intentional platform differences.

## M-02: Account isolation and permission recovery

Goal: prevent stale readings or credentials from one account being shown or used
for another, including while asynchronous reads are completing.

Acceptance criteria:

- [ ] Map selected-account, sign-out, expiry, explicit access refusal and recovery
  states for Codex, Claude, Cursor and Grok Bot on each supported platform.
- [ ] Exercise A-to-B account switching with a late reply from A. Reject that reply
  and prevent cached A data from becoming B's current reading.
- [ ] Exercise concurrent raw/encoded Grok Bot account names, corrupt credential
  stores and expired sessions. Do not select an unrelated account as a fallback.
- [ ] Check that background polling does not raise a macOS permission prompt;
  explicit user action requests access and refusal/recovery updates the UI.
- [ ] Keep credentials and tokens out of logs, fixtures and artifacts. Link passing
  synthetic tests; list real Keychain/Credential Manager and device checks separately.

## M-03: Maintainer automation pilot

Goal: evaluate whether optional OpenAI API assistance reduces recurring maintenance
work without weakening review. This pilot has not been implemented or funded.

Acceptance criteria:

- [ ] Add opt-in scripts that accept a public issue, a public PR diff or a selected
  fixture case and produce a local triage summary, review suggestions, regression
  draft or release-note draft. Keep the first pilot small and manually invoked.
- [ ] Make transmitted inputs inspectable. Use public code and synthetic/redacted
  fixtures; exclude credentials and raw user session logs.
- [ ] Set explicit model, token/cost bounds and timeouts. Record usage and errors;
  failure must leave the ordinary manual workflow usable.
- [ ] Evaluate on fixed, labelled examples. Record factual errors, missed regressions,
  accepted/rejected suggestions, editing time and API cost against a manual baseline.
- [ ] Require maintainer review and the appropriate test suites before accepting
  generated changes. Do not execute instructions found in issue text or model output,
  or automatically merge, publish or change account settings.
- [ ] Publish the scripts, setup instructions and an evidence-based go/no-go result.
  Claim time savings only if the comparison supports them.

## Contributing to these items

Reference M-01, M-02 or M-03 in an issue or pull request. State the platform,
source revision, reproduction/fixture and the acceptance criteria addressed.
Keep each change scoped. The maintainer records review, test evidence and any
remaining device checks before marking an item complete.

© 2026 dydxfx · https://dydxfx.com
