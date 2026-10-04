# Console changelog

## 0.6.0

- Target published Dekopon 0.33.0 with exact core pins; pass the shell tree context through console broker invokers so broker child-script upcalls execute under the shared script budget.
- Add an opt-in real-broker regression: a synthetic provider spawns `printf child`, forwards child stdout, and the console ShellRuntime asserts its output and zero exit status. Requires a verified 0.33.0 broker binary; it is not a production Python/gh-provider test.

## 0.5.0

- Target published Dekopon 0.31.0 with exact crates.io pins for all ten authored core dependencies; CI enforces exact pins and rejects core git/path dependencies.
- Use the streamed broker shell: forward provider stream descriptors, secret intent and command completion, preserve stderr/nonzero exits, and share the cumulative call budget across scripts.
- Forward script lifecycle, cancellation and job control; warn explicitly when an executed provider asset effect cannot be presented locally.
- Add descriptor-wire and shell regression coverage. Real broker/provider and telemetry integration requires separately supplied verified fixtures.

This changelog starts at 0.5.0; earlier releases did not have a repository changelog. `dekopon-tui` remains independently versioned at 0.11.1.
