# Console changelog

## 0.10.0

- Reject authored conversation scopes in console profiles; keep synthetic smoke conversations available without claiming authority over real conversations.
- Show completion status for successful shell commands as well as failures.
- Target published Dekopon 0.36.0 with exact crates.io pins for all ten authored core dependencies.

## 0.9.0

- Add console-only decoded asset intake and reuse local and returned assets across legs of the same agent entry.
- Add a broker-derived synthetic conversation flag and local delivered-turn recording.
- Report broker denials for synthetic records as failed shell commands rather than claiming success.
- Bind synthetic record identifiers and trace parents to the live console process and turn.

## 0.7.0

- Accept broker-returned assets using published core 0.33.0 attachment APIs and a per-agent-leg, volatile 16-item / 64 MiB memory store. Print the registration ID, media type and stored byte count without claiming delivery; remove the old post-effect refusal. No durable persistence or chat transport.
- Exercise real asset registration with a verified v0.33.0 broker and synthetic Wasm provider (red on v0.6.0, green on this branch); retain ordinary stderr and nonzero status behavior.

## 0.6.0

- Target published Dekopon 0.33.0 with exact core pins; pass the shell tree context through console broker invokers so broker child-script upcalls execute under the shared script budget.
- Add an opt-in real-broker regression: a synthetic provider spawns `printf child`, forwards child stdout, and the console ShellRuntime asserts its output and zero exit status. Requires a verified 0.33.0 broker binary; it is not a production Python/gh-provider test.

## 0.5.0

- Target published Dekopon 0.31.0 with exact crates.io pins for all ten authored core dependencies; CI enforces exact pins and rejects core git/path dependencies.
- Use the streamed broker shell: forward provider stream descriptors, secret intent and command completion, preserve stderr/nonzero exits, and share the cumulative call budget across scripts.
- Forward script lifecycle, cancellation and job control; warn explicitly when an executed provider asset effect cannot be presented locally.
- Add descriptor-wire and shell regression coverage. Real broker/provider and telemetry integration requires separately supplied verified fixtures.

This changelog starts at 0.5.0; earlier releases did not have a repository changelog. `dekopon-tui` remains independently versioned at 0.11.1.
