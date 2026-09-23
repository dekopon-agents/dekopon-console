# dekopon-tui

The session and rendering library for [`dekopon-console`](../../README.md). It runs a bounded `dekopon-agent` prompt loop over a `BrokerLeg` connected to an authenticated Unix peer, without broker policy, provider credentials, or a direct Wasm host. The broker decides every capability proposal; this library does not authorize subjects or tool effects. Exact published Dekopon dependency pins are `=0.19.0`; core main `e9800510` has later unpublished model API changes and must not be claimed compatible on this pin.

An agent is orchestration configuration, not a human identity. `OperatorProfile` binds an agent to a typed `ExternalSubject` and optional typed `ChatScopeClaim`; the broker must map/attest the console peer UID and authorize `agent.prompt` and its effective surface. Scope in the UI is a **REQUESTED** claim, not a verified broker echo. The broker's empty-`chatScopes` legacy behavior silently drops it; see the root README's required nonempty attestor grants and missing-trusted-scope Cedar forbid. Neither a picker nor a profile restricts what arbitrary code running as this peer can claim.

The catalog provides safe instructions, already bounded mounted skills, and safe meta-tool metadata; the broker provides actual capabilities/command words. The shared loop's `read_skill` and `inspect_agent_config` are available, while improvement suggestions remain opt-out. `RecordingInvoker` forwards typed `SecretUseProposal`, provider command words, and script completion; it sees model-authored script arguments and provider output for local redacted panes, without becoming a second authorizer. `RecordingProgress` relays bounded tool/model metadata, not model text. `StopFlag` signals both the prompt probe and a per-turn `CancelSignal` in the broker leg. A broker-accepted effect is not rollbackable.

The console has no production history, chat asset input source, delivery sink, or model-facing improvement suggestion sink. References to absent `chat-asset:<N>` inputs are refused before submission; when a provider executes but returns assets the console cannot present, the outcome is an explicit local failure naming the already executed effect and warning not to repeat it. Descriptors are released by the core leg. No fake send/delivery is recorded. See the root README for credential isolation and source-backed broker integration fixture setup.

Every drawn field passes through terminal-control sanitization. Provider/model payloads are redacted at render time; `r` deliberately reveals one field of one selected call and warns that it now lives in scrollback. The terminal guard restores raw mode and the alternate screen on exit/error/panic. The local shell pane uses Dekopon's bounded interpreter, not a container OS shell.

| Key | Action |
|---|---|
| `tab` / `shift-tab` | next / previous pane |
| `j` `k` / arrows | move agent or call cursor |
| `enter` | enter selected agent (confirm requested scope when prompted) |
| `i`, then `enter` | compose and submit a turn or shell line |
| `esc` | stop at a cooperative boundary or leave composer |
| `o` / `r` | expand call / reveal one redacted field |
| `?` / `q` | help / quit |

Licensed under either [Apache-2.0](../../LICENSE-APACHE) or [MIT](../../LICENSE-MIT).
