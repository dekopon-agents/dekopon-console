# Console simplification ledger

Canonical brief: core repository `SIMPLIFY-BRIEF.md` on `simplify/2026-09` (initial
amendment `5a8687c`, residue gate `2010f55`). The sibling integration worktree
`../simplify/` contains that brief. Read it and this ledger; no external correction
addendum or prior conversation is required.

This repository is `dekopon-agents/dekopon-console`; base `ef0bf3f`, integration branch
`simplify/2026-09`. Keep all published Dekopon dependencies pinned to `=0.11.1`.
No publication, tags, releases or merges. Core runner retirement waits for both units.
One worker and one commit per unit; fresh high-effort review; package tests and UI
redaction/terminal lifecycle gates. Reclaim inactive ignored targets at handoff.

## D8a
landed — freshly reviewed and cherry-picked into `simplify/2026-09`; development-only 0600 local-socket chat client with bounded ordered exchanges,
no model/credential setup, and no-TTY safe UI tests; commit: `Simplify-Unit: D8a`;
discoveries: none. Review repair: bare relative socket parent fallback and payload-free
I/O kind/errno display, with real-listener and no-TTY regressions. Validation: package check/test, clippy, fmt, documentation and
pin gates; evidence: `.validation/simplify-2026-09/D8a/` in the enclosing workspace.

Reviewed worker commit: `b5aa18afbe862ff48c85ac9a6c209b85fc4a6873`; fresh review:
workspace `.validation/simplify-2026-09/D8a/rereview.md`. Integration self identity:
`Simplify-Unit: D8a` (resolve its integrated SHA in the next work metadata).
Only this ledger differs from the reviewed tree. Sequential integrated package check/test
(91 tests, zero failed/ignored), Clippy, fmt, doctest/rustdoc, documentation, metadata,
privilege and exact published-pin gates passed; final-head receipts and push results are
in workspace `.validation/simplify-2026-09/wave0-pair1-integrated.md`. No CI or physical-TTY
pass is claimed. Integration target is retained only for immediate D8b/remaining wave-0
gates; remove when those gates finish or if headroom falls below the next width budget.
Worker target was
already absent at integration; the registered source worktree stays to the wave boundary.

## D8b
landed — freshly reviewed and integrated into `simplify/2026-09`.
Reviewed worker: `943636e6c60bd3635d4447404d6f5616709f7551`; review:
workspace `.validation/simplify-2026-09/D8b/review.md`. Only integration ledger metadata
differs from the reviewed tree. Batch exact-head gates and cleanup receipts are recorded
in workspace `.validation/simplify-2026-09/wave0-batch-2.md`.
Self identity: `Simplify-Unit: D8b`, branch `simplify/unit-D8b` (resolve the exact work SHA
at integration). Existing broker-backed shell starts with `--shell` / `ModelChoice::ShellOnly`
without model credential path/file/env resolution or model construction. Model flags conflict;
turn composition, submission and dispatch stay disabled across pane/agent changes. No second
REPL; interpreter, broker authorization, pipeline/exit rendering and D8a chat are preserved.

Prior integrated identities verified at dispatch: console/D8a
`97e8625617aa44dbc4b62a58d6f005b98d0c7af6` (reviewed worker
`b5aa18afbe862ff48c85ac9a6c209b85fc4a6873`); known core batch head
`302cff4922c189a7783248ce159764a29765c522`. D8b fast-forwarded its clean branch
from `9b6c0685d275d44a0238c28d696ee5e83ee49223` to that console head before writing.

Validation: package check/tests, stable and Rust 1.89 Clippy, fmt, doctest/rustdoc,
CLI help/conflicts, poisoned-credential subprocesses, detached real-binary startup,
real-socket attested shell proposal/denial and normal loopback-model turn, TestBackend UI,
seven exact published pins, documentation and corrected mechanical gates. Existing tests and
assertions remain unchanged; no public identifier, crate, binary, file or test is removed.
Evidence: workspace `.validation/simplify-2026-09/D8b/report.md`. Discoveries: none outside scope.
No CI or physical-TTY pass claimed. Coordinator removed the exact inactive ignored worker
target immediately after cherry-pick (3.9 GiB logical; physical free 73.38 to 77.22 GiB).
Registered worker source stays until the wave boundary. Integration target is retained only
for the immediate full wave-0 console gate; reclaim once that gate finishes or for headroom.

## Commit identity

Each work commit updates its own entry and carries `Simplify-Unit: <ID>`. The next
integration update resolves the prior entry's exact SHA; a commit cannot store its own
hash. Remove this ledger only in final housekeeping before the console PR.
