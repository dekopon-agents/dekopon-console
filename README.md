# dekopon-console

An interactive terminal view over a running [Dekopon](https://github.com/dekopon-agents/dekopon)
broker. Pick an agent, see what policy actually grants it, type a turn, and watch every capability
call go out with its arguments and its result.

```console
cargo install --locked dekopon-console
dekopon-console --subject dev.console.xavier
```

It moved out of the dekopon tree the way
[dekopon-provider-gh](https://github.com/dekopon-agents/dekopon-provider-gh) did, and for the same
reason: it is an ordinary client of published crates, not part of the control plane. Taking it out
dropped a TUI framework and five duplicate-version dependency exemptions from a repository whose job
is authorization.

## What it is, and what it is not

It is an **unprivileged client of `dekopon-brokerd`'s Unix socket**. It holds a model credential and
nothing else: no policy, no provider credential, no authorization, no component host. Every
capability call it makes is a proposal, and the broker alone decides it.

It runs the agent loop itself, which is not a preference. The broker has no model client and no
concept of a turn; in a deployment `dekopond` runs the loop and the broker authorizes what the loop
reaches. This takes that gateway role for one operator at one terminal — and that is the only reason
it can show a tool call's arguments and result at all. Conversation history keeps the prompt and the
answer, `shell.command` spans keep an argument count, and the audit chain keeps digests. Those
values exist nowhere but the process running the loop.

## Render-time redaction is a security property

Everything the console draws passes through `dekopon-tui`'s `redact` module before it reaches a
buffer, and both halves of that matter:

- **Terminal-control sanitisation.** A pull-request title and an issue body are attacker-controlled
  text arriving through a read-only capability. Drawn raw they can move the cursor, repaint earlier
  lines, or reorder text through a bidirectional override. Nothing reaches a buffer another way.
- **Per-field secret redaction.** What a model wrote or a provider returned can carry a token, and
  a shape match hides only the run that matched so the sentence around it survives. Revealing is
  one keystroke against one field — `r` uncovers the next hidden field of the call under the cursor
  and says in the status line that it is now in your scrollback. It is never a mode.

Provider credentials are not in this data by construction: the broker injects those inside its own
native HTTP engine, after guest-header validation, where no client can observe them.

## Flags

| Flag | Meaning |
|---|---|
| `--subject <SUBJECT>` | canonical external subject sessions propose on behalf of; also `DEKOPON_CONSOLE_SUBJECT`. No default |
| `--config <PATH>` | agent catalog; resolved the way `dekopon` resolves it when absent |
| `--socket <PATH>` | broker socket; then `$DEKOPON_BROKER_SOCKET`, `$XDG_RUNTIME_DIR/dekopon/broker.sock`, `$HOME/.local/run/dekopon/broker.sock` |
| `--server-uid <UID>` | trusted UID owning the broker process; defaults to the caller's own |
| `--model <MODEL>` | model name handed to the backend |
| `--auth-file <PATH>` | ChatGPT credential file; conflicts with `--endpoint` |
| `--endpoint <URL>` | OpenAI-compatible endpoint instead of the ChatGPT subscription |
| `--api-key-env <NAME>` | variable holding that endpoint's bearer token; requires `--endpoint` |
| `--max-steps <COUNT>` | model turns one session may take |
| `--max-capability-calls <COUNT>` | capability invocations one session may drive |

`--subject` grants nothing by itself. The broker still needs an attestor grant covering that
namespace and an owner-authored `identityMappings` entry, or it resolves to nothing.

The console keeps its own `chatgpt-auth.console.json` rather than the `chatgpt-auth.json` every
other surface resolves to. The refresh token rotates, so whichever process refreshes invalidates
every other copy — including one exported into a secret store. Run
`dekopon auth chatgpt login --auth-file <PATH>` once.

## Version pinning

Both crates depend on **exactly `=0.11.1`** of every dekopon crate they use, from crates.io. Not a
caret range: these are pre-1.0, so `0.11.2` would be admitted automatically, and the broker protocol
is a wire contract this console has not been run against. Not a git dependency either — the `gh`
provider pinned a branch once, and a `branch =` dependency resolves by fetching the ref, so a
deleted branch broke every cold-cache build.

`dev.<surface>.<name>` subjects are part of that pin. Dekopon removed the `dev.*` subject service
and the broker's `allowDevelopmentSubjects` opt-in when this console left its tree, so a broker
built from dekopon `main` refuses one as an unknown service. Against such a broker, map an ordinary
subject instead.

## Layout

- `crates/dekopon-tui` — the library: state machine, session driving, observation decorators,
  redaction, panes. Published under the name it already had in the dekopon workspace, so its
  crates.io lineage continues rather than restarting.
- `crates/dekopon-console` — the binary: one command line, and nothing else.

## License

Licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.

## Development gateway chat

`dekopon-console --subject tel.15550100000 --chat-socket /path/to/local.sock --conversation my-chat`
opens a text-only client of an already-running `dekopond` local transport. Chat skips catalog,
broker-client, model and credential setup entirely. It sends one JSON request line and waits for
one response; no tool loop runs in this mode. The socket must be same-owner 0600, with a same-UID
peer and a parent not writable by other users. The UI always warns that the caller declares the
subject: **development only**, not production authentication. The gateway still routes and the
broker still authorizes it.

Enter sends, Esc clears the composer, Ctrl-C quits. Only the latest reply is displayed, redacted
and terminal-sanitized. Images are not displayed. Requests and replies are capped at 64 KiB
including the JSON envelope and newline; exchanges time out after 120 seconds. Failed exchanges
close the connection and require restarting chat; there is no automatic retry. Quitting or timing
out does not cancel gateway work. `--conversation` defaults to `dev`; use a distinct value for a
separate conversation, or intentionally reuse it to resume gateway-managed history.
