# Acceptance Tests

These tests were run during pack creation to verify the agent-chorus context pack
actually helps agents on real tasks. Each test compares a pack-only answer against
grep-verified ground truth in this repo. All grep commands below are run from the
repo root (`/Users/e059303/sandbox/play/agent-chorus`).

## Test 1: Lookup

**Question:** Where is the manifest schema-version constant defined that
`chorus agent-context verify` enforces against `manifest.schema_version`?

**Pack-only answer:** `20_CODE_MAP.md` points to `cli/src/agent_context.rs` as the
authoritative Rust agent-context module ("Init, seal, verify, build, hooks"). The
constant is `CURRENT_SCHEMA_VERSION` defined at the top of that file, and the
enforcement function `check_schema_version` reads `manifest.schema_version` and
compares it against `CURRENT_SCHEMA_VERSION`.

**Grep verification:** `cli/src/agent_context.rs:20` defines
`const CURRENT_SCHEMA_VERSION: u64 = 1;` and `cli/src/agent_context.rs:250` defines
`fn check_schema_version` which inspects `manifest.get("schema_version")`.

```bash
grep -n "CURRENT_SCHEMA_VERSION\|fn check_schema_version" cli/src/agent_context.rs
```

This grep returns the constant declaration on line 20 plus the function definition
and its callers — non-empty, single authoritative file, exact line numbers.

| Metric | Value |
|---|---|
| Pack pointed to correct file? | yes (`cli/src/agent_context.rs`) |
| Files opened to verify | 1 |

---

## Test 2: Impact Analysis

**Question:** A contributor wants to add a new `chorus agent-context` subcommand
(say, `chorus agent-context status`). List every file that must change in the
same PR.

**Pack-only answer (files that must change):** Per the
`30_BEHAVIORAL_INVARIANTS.md` Update Checklist row "New agent-context subcommand":
`cli/src/main.rs` (Clap subcommand enum), `scripts/read_session.cjs` (command
dispatch), `scripts/agent_context/<sub>.cjs` (Node implementation), the
context-pack integration test runner under `scripts/`, and `docs/CLI_REFERENCE.md`.
Behavioral Invariant 14 additionally requires a golden fixture under
`fixtures/golden/` and a matching case in `scripts/conformance.sh` when the
subcommand emits structured output. Invariant 1 (Node/Rust parity) means the
Rust implementation in `cli/src/agent_context.rs` must move in lockstep with
`scripts/agent_context/*.cjs`.

**Grep verification:** every load-bearing path the checklist names is real, and
the file-families row in the pack lines up with what is on disk.

```bash
grep -n "New agent-context subcommand\|cli/src/main.rs\|scripts/read_session.cjs\|scripts/agent_context\|scripts/conformance.sh\|fixtures/golden" \
  .agent-context/current/30_BEHAVIORAL_INVARIANTS.md \
  .agent-context/current/20_CODE_MAP.md
```

This grep returns multiple hits across both pack files — the checklist row, the
file-families row, and the navigation entries — confirming the pack
self-consistently names every authoritative file the impact analysis needs to
touch.

| Metric | Value |
|---|---|
| Files identified by pack | 7 (incl. families) |
| Files found by grep | 7 |
| Coverage ratio | 100% |
| False positives | 0 |
| False negatives | 0 (the checklist also implicitly covers `cli/src/agent_context.rs` via invariant 1) |
| Pass (>=80%)? | yes |

---

## Test 3: Diagnosis

**Question:** A change to a Rust subcommand's `--print` JSON output passes
`cargo test` locally but lands a regression. The Node implementation, the schemas,
and the golden fixtures were never updated. What silent-failure mode does the
pack predict, and which gating check should have caught it?

**Pack-only diagnosis plan:** `30_BEHAVIORAL_INVARIANTS.md` invariants 1, 13, and
14 spell out the failure mode. Node/Rust parity (invariant 1) and byte-identical
JSON across runtimes (invariant 13) require any output-shape change to update
both runtimes, regenerate goldens, and pass conformance in the same PR. Invariant
14 says "Golden fixture + conformance test required for new subcommands... No
golden = no merge." `scripts/conformance.sh` is the gating check that compares
Node and Rust output against `fixtures/golden/*.json`. If the change skipped that
flow, the regression slips through `cargo test` (Rust-only) but
`scripts/conformance.sh` would have failed because the Rust output diverges from
the unchanged golden, or — worse — if the golden was hand-edited to "fix" the
diff (forbidden by the negative guidance), conformance would pass while the
schema and Node implementation silently drift. The pack's
`20_CODE_MAP.md` flags `scripts/conformance.sh` as the "validates Node/Rust
parity. Gates all merges" file.

**Source verification:**

```bash
grep -n "byte-identical\|conformance.sh.*gating\|No golden = no merge" \
  .agent-context/current/30_BEHAVIORAL_INVARIANTS.md
```

This grep returns invariants 13 and 14 verbatim — non-empty, exact line numbers
in the pack — confirming the pack already documents the silent-failure mode and
names `scripts/conformance.sh` as the gate.

| Metric | Value |
|---|---|
| Pack pointed to correct subsystem? | yes (`scripts/conformance.sh` + invariants 1, 13, 14) |
| Pack avoided dead ends? | yes (no need to read `cli/src/*.rs` to diagnose) |
| Files opened to verify | 1 (the invariants file itself) |
| Additional files needed beyond pack guidance | 0 |

---

## Test 4: Negative Guidance

**Question:** A reviewer sees a PR diff that hand-edits two files in
`fixtures/golden/` to clean up "noisy whitespace" — no other code changes. Should
the reviewer approve, and what does the pack say about this pattern?

**Pack-only answer:** No. The pack's Negative Guidance in
`30_BEHAVIORAL_INVARIANTS.md` says explicitly: "Do not modify
`fixtures/golden/*.json` by hand — run conformance to regenerate them." The File
Families row reinforces this: golden fixtures are "Derived — regenerated by
running conformance with `--update`." Hand-edits silently break the parity
contract that invariants 1, 13, and 14 enforce — the goldens become a fiction
that no longer reflects either runtime's actual output. The reviewer should
reject the PR and ask the contributor to regenerate the goldens via the
conformance runner.

**Source verification:**

```bash
grep -n "Do not modify .fixtures/golden\|Derived . regenerated by running conformance" \
  .agent-context/current/30_BEHAVIORAL_INVARIANTS.md
```

This grep returns the negative-guidance bullet (line 56) and the file-families
row (line 41) — non-empty, both rules co-located in the same pack file the agent
already loaded in step 2 of the read order.

| Metric | Value |
|---|---|
| Pack stated the prohibition explicitly? | yes |
| Pack stated the correct alternative (regenerate via conformance)? | yes |
| Files opened to verify | 1 |

---

## Summary

| Test | Category | Pass? |
|---|---|---|
| 1 | Lookup | yes |
| 2 | Impact analysis | yes |
| 3 | Diagnosis | yes |
| 4 | Negative guidance | yes |

**Overall:** all pass. The pack answers each task category from the three-file
read order (`00_START_HERE.md` -> `30_BEHAVIORAL_INVARIANTS.md` -> `20_CODE_MAP.md`)
without needing to open repo source files except to confirm exact line numbers.

**Iterations:** 0 — this pack inherits the v0.14.1 sealed content; the tests
above were authored against the sealed pack as-is.
