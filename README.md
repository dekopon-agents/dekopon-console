# dekopon-console

An operator-controlled interactive client of `dekopon-brokerd`. This branch implements the console session milestone, **not** packaging, chart enablement, or deployment. The broker alone maps subjects, authenticates the Unix peer UID, checks `agent.prompt` and provider grants, and holds provider credentials. Whoever can exec arbitrary code as the console UID can claim any subject/scope inside its broker attestor envelope; the profile picker is convenience, not authorization or Kubernetes user attribution.

## Run

```sh
dekopon-console --config /config/dekopon.yaml --profiles /config/console-profiles.yaml \
  --socket /run/dekopon/broker.sock --server-uid 65532
# exact catalog name plus an authored matching profile
dekopon-console --config /config/dekopon.yaml --profiles /config/console-profiles.yaml \
  --profile family-channel --agent lange-family --server-uid 65532
# explicitly subject-only, for a separately authorized deployment
dekopon-console --subject slack.t0123abc.u9xyz --agent reviewer --server-uid 65532
# separate idle PID1; no catalog, broker, model, or credential setup
dekopon-console --idle
```

Interactive commands need stdin/stdout TTY (`kubectl exec -it`). A missing broker, incorrect server UID, refused peer/agent, unknown agent, missing profile, ambiguous same-agent profiles, or absent TTY fails clearly. The picker still shows every catalog agent; selecting one with no matching profile refuses rather than borrowing another agent's identity. An empty broker surface and a failed fresh `agent.prompt` gate prevent model inference. An idle process never makes a model or broker request.

### Authored profiles

This strict YAML or JSON document is **read-only, credential-free operator data**. Only `defaultProfile`, when explicitly present, selects the bare invocation's subject; bare launch still opens the picker. `--profile` selects a bound agent and may not be combined with `--subject`. `--agent` selects an exact catalog name; if multiple profiles bind it, pass `--profile`. A profile bound to another agent is a refusal. Without an authored default/selected profile, an explicit `--subject` (or `DEKOPON_CONSOLE_SUBJECT`) is required. Unknown keys, duplicate names, unknown catalog agents, invalid scope, zero or excessive limits refuse before broker/model setup. Profile names use ASCII letters/digits/`-`/`_`; `maxSteps` is 1–64 and `maxCapabilityCalls` is 1–256. CLI `--model`, `--max-steps`, `--max-capability-calls` override profile fields; then the fallback is model `gpt-5.6-luna`, 8 steps, 16 calls. The selected model is always visible in the header. Keep the full credential-free catalog and referenced skill paths available; the catalog loader refuses missing skills.

```yaml
defaultProfile: family-channel
profiles:
  - name: family-channel
    agent: lange-family
    subject: slack.t0123abc.u9xyz
    scope: # exact published ChatScopeClaim shape; no parallel scope grammar
      transport: operator
      kind: slack
      conversation: {kind: directMessage, container: t0123abc, id: d0123abc}
    model: gpt-5.6-luna
    maxSteps: 8
    maxCapabilityCalls: 16
```

**Requested scope is not broker-confirmed.** The 0.19 broker wire does not echo authenticated chat scope; an attestor with empty `chatScopes` silently downgrades a scoped request to subject-only. The console displays an explicit confirmation before scoped entry and marks the header REQUESTED. A safe route-context deployment must configure **nonempty exact console `chatScopes` AND Cedar fail-closed for the console `via` whenever trusted transport/conversation is absent**, preferably an explicit forbid. Test that deleting `chatScopes` refuses `agent.prompt` and tools, and that mismatched conversations/agents/subjects refuse. All staged homelab profiles for this milestone must be concrete route-context profiles; subject-only mode remains for other deliberately authorized installations. Do not reuse the gateway's UID or spoof its `via`.

### Model and data limits

The published `=0.19.0` model clients support a private ChatGPT subscription `--auth-file` (`chatgpt-auth.console.json` by default), or `--endpoint URL` plus optional `--api-key-env NAME` for OpenAI-compatible endpoints. A named but absent/empty bearer variable refuses **when a model turn is attempted**, not while browsing. The console credential must not share the gateway's rotating refresh-token family; explicit `--auth-file` overrides the shared-file guard at the operator's risk. The chart should reference a separately owned API-key Secret or isolated subscription credential state, never mount broker/gateway credentials. No model is contacted for catalog browsing, empty grants or idle. `--server-uid` defaults to local effective UID only for same-owner development; the separate console UID 65535 / broker UID 65532 deployment must pass 65532 and have IPC group 65534, without write access to the socket parent.

The console uses the released shared prompt loop, mounted `read_skill`, safe `inspect_agent_config`, provider command words and help, progress metadata and cooperative broker-command cancellation. Stop reaches both the current model turn's broker command leg and a manual shell command, but does **not** undo effects already accepted. The capability pane shows the current broker leg’s W3C trace ID; on submission the fresh leg adopts the console turn’s trace. Each model/manual turn creates a fresh console root; shared model, script, shell, broker and provider spans inherit it through async and blocking work. Without export configured this is local context correlation only, not durable telemetry evidence. Model arguments/results appear in local terminal panes; reveal can expose secrets into scrollback. Fresh agent entry resets transcript, shell state and model history. Manual scripts are Dekopon's bounded interpreter, not OS shell. It does not synthesize inbound delivery, chat replies, production transcript replay, or conversation memory. `suggest_improvement` remains deliberately off (not enabled by the approved session contract); model routing by core #321/OpenRouter is not present in published 0.19. Text-only console inputs have no local asset source; `chat-asset:<N>` proposals are refused before broker submission. On provider asset effects, core drops returned descriptors and reports a note; the console turns that outcome into an explicit **failed local presentation** warning after the effect executed, never reports fake delivery. Binary asset intake/presentation is a follow-up, not parity. This release must not be marketed as full image or transport replay.

## Dependency / verification boundary

Exact crates.io `=0.19.0` Dekopon pins, Rust 1.98.1, and committed Cargo.lock. Core main `e9800510980652498d5c4dff14bcf1a414ff215c` merged model API changes **after** published 0.19.0 without changing its workspace version. A new approved publication is required to adopt #321 model APIs; published 0.19.0 cannot substantiate #321 support. To exercise the broker boundary without live requests, set `DEKOPON_TEST_BROKERD` to a separately verified broker executable and `DEKOPON_TEST_PROBE_WASM` to core's pure `cli-probe-provider.wasm`, then run `cargo test -p dekopon-tui --test broker --locked`. The test skips only when those variables are absent; a submitted compatibility claim must provide them and record the artifact/ref tested. It tests attested/denied/empty surfaces, UID pin, chat-scope downgrade, scoped mismatch, provider help, pure read, ungranted call, and typed secret-use denial. For this milestone the same test also passed against a separately built broker from read-only core Git archive `e9800510980652498d5c4dff14bcf1a414ff215c` (broker binary SHA-256 `f7be0df04091a4f2688b3953d16cc299fded30580f75da9199b378cbc2ca9c23`) using the checked `cli-probe` component SHA-256 `091ca1a26e3fbb5aefef517c74e4f9635f2fbb05e4eafe21d5ccb2f42a66cba2`. This proves the tested broker wire/fixture paths, **not** unpublished #321 model API compatibility or untested provider effects. No path/git dependency is committed.

The [`dekopon-tui` crate](crates/dekopon-tui/README.md) owns rendering, transcript, and terminal state. Copyright licensed under [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT).


### Console telemetry

`--telemetry /etc/dekopon/console-telemetry.yaml` enables both OTLP traces and correlated logs.
No telemetry network/export occurs by default; `--idle` ignores this option and does not read
its file or initialize exporters. Interactive execution always maintains local OTel context so
broker proposals carry the actual turn trace even without console export. Diagnostics go only
to redirected stderr (never into the TUI); orderly quit/refusal flushes and shuts down exporters.

The strict YAML file is a console-owned serialization mirroring core daemon telemetry keys,
validated by published `dekopon_telemetry::ExporterSettings` (that type is not deserializable):

```yaml
endpoint: https://telemetry.example.invalid/otlp
transport: http                 # grpc (default) or http
serviceName: dekopon-console     # default
exportTimeoutMs: 5000            # default, positive; export and shutdown deadline
```

`endpoint` is required and must be a credential-free base URL without query/fragment/userinfo.
Unknown keys and invalid settings fail startup with safe diagnostics. Ingestion authentication
uses only the SDK’s existing `OTEL_EXPORTER_OTLP_HEADERS` environment convention, never YAML
or command-line credential values. Do not share gateway credential/state mounts. A future chart
lane may mount this small read-only file at the example path; no chart value/mount or deployment
is added here. Its configured receiver must be inside the operator trust boundary: telemetry
contains complete prompts, answers, scripts, arguments and results under the normal shared
core payload rules, excluding secret bytes and model/OTLP/provider credentials. There is no
metadata-only/payload suppression switch. The collector/broker must both export to reconstruct
the whole trace; no traceparent is sent to external model endpoints.

Model routing is persistently labelled `model (CLI override)`, `model (profile)`, or
`model (default)` on its own header row, including after agent/profile switches. An explicit
`--model` wins over authored profile models; this is not an exact reproduction of that profile.

The trace regression in `crates/dekopon-tui/tests/support/trace.rs` uses a loopback synthetic
OpenAI-compatible server, OTLP protobuf collector, real broker process and pure `cli-probe`
Wasm. With the fixture variables below, it checks two distinct turn traces, causal parentage
through model/script/broker/provider, full correlated synthetic payloads and credential exclusion.
It never calls a live model/provider. Fixture-less runs explicitly skip broker integration:

```sh
DEKOPON_TEST_BROKERD=/path/to/verified/dekopon-brokerd \
DEKOPON_TEST_PROBE_WASM=/path/to/verified/cli-probe-provider.wasm \
DEKOPON_TEST_ASSET_WASM=/path/to/verified/http-probe-provider.wasm \
cargo test --workspace --locked -- --test-threads=1
```
