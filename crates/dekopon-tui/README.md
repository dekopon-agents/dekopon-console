# dekopon-tui

The session and rendering library for [`dekopon-console`](../../README.md). It runs a bounded `dekopon-agent` prompt loop over a `BrokerLeg` connected to an authenticated Unix peer, without broker policy, provider credentials, or a direct Wasm host. The broker decides every capability proposal; this library does not authorize subjects or tool effects. Exact published Dekopon dependency pins are `=0.22.0`.

An agent is orchestration configuration, not a human identity. `OperatorProfile` binds an agent to a typed `ExternalSubject` and optional typed `ChatScopeClaim`; the broker must map/attest the console peer UID and authorize `agent.prompt` and its effective surface. Scope in the UI is a **REQUESTED** claim, not a verified broker echo. Conversation narrowing lives in broker Cedar policy; see the root README. Neither a picker nor a profile restricts what arbitrary code running as this peer can claim.

The catalog provides safe instructions, already bounded mounted skills, and safe meta-tool metadata; the broker provides actual capabilities/command words. The shared loop's `read_skill` and `inspect_agent_config` are available, while improvement suggestions remain opt-out. `RecordingInvoker` forwards typed `SecretUseProposal`, provider command words, and script completion; it sees model-authored script arguments and provider output for local redacted panes, without becoming a second authorizer. `RecordingProgress` relays bounded tool/model metadata, not model text. `StopFlag` signals both the prompt probe and a per-turn `CancelSignal` in the broker leg. A broker-accepted effect is not rollbackable.

The console has no production history, chat asset input source, delivery sink, or model-facing improvement suggestion sink. References to absent `chat-asset:<N>` inputs are refused before submission; when a provider executes but returns assets the console cannot present, the outcome is an explicit local failure naming the already executed effect and warning not to repeat it. Descriptors are released by the core leg. No fake send/delivery is recorded. See the root README for credential isolation and source-backed broker integration fixture setup.

Every drawn field passes through terminal-control sanitization. The manual shell transcript is sanitised at render time; model/session machinery remains in the library but model turns and the call-tree pane are not exposed by this console UI. The terminal guard restores raw mode and the alternate screen on exit/error/panic. Entering an agent opens the shell ready to type. Commands and bounded interpreter results appear above the editable trailing prompt in a single wrapped viewport. Nonzero exits and output truncation are marked. Each command is a separate script; variables do not carry over. This is Dekopon's bounded interpreter, not a container OS shell.

| Key | Action |
|---|---|
| `tab` / `shift-tab` | next / previous pane |
| `j` `k` / arrows | move agent selection |
| `enter` | enter selected agent (confirm requested scope when prompted); in shell, run command |
| `i` | resume typing in shell after leaving input |
| `page up` / `page down` | page wrapped shell rows, including while typing |
| `home` / `end` | jump to beginning / follow bottom of shell transcript |
| `esc` | request cooperative stop when busy; otherwise leave input |

| `?` / `q` | help / quit when not typing |

On compact Mac keyboards, Fn-Up/Down and Fn-Left/Right usually deliver PageUp/PageDown and Home/End. Terminal emulators can intercept these keys; Command-key shortcuts are not guaranteed to reach the app. Tab/Shift-Tab leave input and switch among Agents, Shell, Capabilities.

Licensed under either [Apache-2.0](../../LICENSE-APACHE) or [MIT](../../LICENSE-MIT).
