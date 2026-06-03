# Native Cursor + Hermes Adapter — Spec, Plan & Task Decomposition

Status: DRAFT / ready to execute
Owner (architect/integrator): Claude (main session)
Implementers (per-unit): `cursor-agent` running `composer-2.5` in auto-run (`--force`)
Repo: `~/sandbox/play/agent-chorus` (binary crate `chorus`, no lib target)

---

## 0. Goals & non-goals

**Goal (primary):** Make Cursor a **first-class agent in `chorus`, at parity with Codex/Claude/Gemini** — `read`, `list`, `search`, `summary`, `timeline`, `diff`, `doctor`, `--cwd` scoping, `--include-user`, `--tool-calls` — by reading Cursor's real CLI transcripts directly. This **deletes the external bridge** (`~/.local/bin/cursor-chorus-bridge`, the `~/.zshenv` routing, and the `chorus` shell wrapper).

**Goal (stretch, only if Cursor is fully done):** Add a **Hermes** agent adapter, wired into every agent enumeration so the binary recognizes it. **Do NOT test Hermes** — Hermes is not installed and its on-disk format is unconfirmed. Bar = compiles, is registered everywhere, returns clean "no sessions" when its data dir is absent.

**Non-goals:** Publishing/releasing the crate (separate confirm-tier step w/ `cargo audit`). Changing other agents' behavior. Supporting Cursor's legacy SQLite (`state.vscdb`) store.

---

## 1. Current state (why the bridge is a stopgap)

- `chorus` 0.14.1's cursor reader (`cli/src/agents.rs`) scans `~/Library/Application Support/Cursor/User/workspaceStorage` for `*.json|*.jsonl` whose names contain `chat`/`composer`/`conversation`, lines shaped `{"role","content":"<string>"}`. Cursor stores chat in SQLite now, so this finds nothing.
- The `cursor-agent` CLI **does** write plaintext JSONL transcripts to `~/.cursor/projects/<project>/agent-transcripts/<session-id>/<session-id>.jsonl`, but in a different shape (see §4).
- `read_cursor_session_with_options` **ignores `cwd`** (param is `_cwd`) and emits a "no project scoping" warning; `list_cursor_sessions` fakes scoping by substring-matching the cwd path in file content.
- We built an external Python **bridge** that transforms those transcripts into the legacy shape/location and routes `$CHORUS_CURSOR_DATA_DIR` per-context via the shell. It works but is scaffolding: duplicated storage, duplicated redaction, a synthetic metadata line, a `chorus` shell wrapper shadowing the binary, and snapshot staleness.

This spec replaces all of that with a native adapter.

---

## 2. Target architecture

`chorus` already has the adapter pattern (`cli/src/adapters/mod.rs`):

```
trait AgentAdapter { read_session_with_options(...); list_sessions(...); search_sessions(...); }
get_adapter("cursor") -> CursorAdapter  // already exists, delegates to agents::*
```

The Codex/Claude readers use this contract:
```
find_latest_by_cwd(&files, &expected_cwd, get_<agent>_session_cwd)   // cli/src/agents.rs:1366
```
where `get_<agent>_session_cwd(path) -> Option<PathBuf>` extracts the session's recorded cwd, and `cwd_matches_project` compares. **Redaction (`redact_sensitive_text`, agents.rs:1628) is already applied to cursor reads** — 10 patterns (OpenAI/AWS/GitHub/Google/Slack/bearer/JWT/PEM/conn-strings/secret-assignments), a superset of the bridge's.

**Design:** keep the cursor `read`/`list`/`search` orchestration **inside `agents.rs`** (so it keeps reusing the private helpers `collect_matching_files`, `select_conversation_turns`, `file_modified_iso`, `redact_sensitive_text`, `find_latest_by_cwd`, `cwd_matches_project`, `ConversationTurn`, `Session`). Delegate only the two genuinely-new, pure sub-problems to **new standalone modules**:

- `cursor_cwd.rs` — resolve a transcript's real workspace cwd.
- `cursor_parse.rs` — flatten a Cursor transcript into `(role, text)` turns.

This minimizes churn in shared files (elegant) **and** isolates the new logic into files a separate implementer can own without races (parallel-safe).

---

## 3. The cwd parity finding (and its honest cap)

Codex/Claude embed cwd **in the session data** (codex: `payload.cwd` on line 1; claude: `cwd` per line) — ground truth, always present. **Cursor does not.** Two recoverable sources, used as tiers:

1. **Authoritative:** `~/.cursor/projects/<project>/.workspace-trusted` → `"workspacePath"`. Exact real path. **Present only for projects trusted via `--trust` (~18/95 here).** We create headless threads with `--trust`, so our own sessions get it.
2. **Fallback (filesystem-validated demangle):** Cursor encodes the originating path in the project dir name with `/`→`-` (`Users-e059303-sandbox-play-big-berlin-hack`). Because real dir names can contain `-`, the name is ambiguous (`play/foo` vs `play-foo`). Disambiguate by **walking the real filesystem** from `/`, backtracking over how many dash-tokens form each existing directory. Returns the real path or `None`.

Once each session has a derived cwd, `find_latest_by_cwd`/`cwd_matches_project` give Cursor the same `--cwd` scoping as the others — and temp/numeric/foreign projects are **naturally excluded** because their cwd won't match the sandbox `--cwd` (no special-case "scope filter" needed; cleaner than the bridge).

**Honest cap:** for a session that was never `--trust`ed **and** whose workspace dir no longer exists on disk, cwd is unknowable → it won't match a `--cwd` filter (still readable by `--id`). This is a Cursor data-model limitation, not a chorus bug. Document it.

---

## 4. Data formats (EXACT — implementers must match these)

### 4a. Cursor transcript line (`~/.cursor/projects/<project>/agent-transcripts/<session>/<session>.jsonl`)

One JSON object per line. `message` is an **object** with a `content` **array** of typed segments. Keep only `type == "text"` segments; ignore `tool_use` etc.

User line (real bytes):
```json
{"role":"user","message":{"content":[{"type":"text","text":"<user_query>\nSummarize what the agent-chorus CLI does...\n</user_query>"}]}}
```
Assistant line (real bytes, truncated):
```json
{"role":"assistant","message":{"content":[{"type":"text","text":"Gathering ... docs.\n\n[REDACTED]"},{"type":"tool_use","name":"Read","input":{"path":"..."}},{"type":"tool_use","name":"Grep","input":{"pattern":"..."}}]}}
```
Rules:
- Roles other than `user`/`assistant` → skip the line.
- Text = concatenation of `text` fields of segments where `type=="text"`, in order.
- Defensive: if `message` is a JSON **string**, treat that string as the text; if `message.content` is a **string**, use it directly. (Real data is always the object+array form above; these are just safety nets.)
- Empty flattened text → skip the turn.

### 4b. `.workspace-trusted` (real bytes)
```json
{"trustedAt":"2026-06-02T19:33:37.491Z","workspacePath":"/Users/e059303/sandbox/work","trustMethod":"cli-flag"}
```
Read `workspacePath` (string). May be absent or the file may not exist.

### 4c. Project dir name mangling
`/Users/e059303/sandbox/work/trust-stream/trust-stream-backend`  →  `Users-e059303-sandbox-work-trust-stream-trust-stream-backend` (every `/` becomes `-`; real dir names like `trust-stream` keep their own `-`). Demangle by fs-walk (see §5 Unit A).

---

## 5. Decomposition into composable units

**Race-avoidance principle:** each parallel implementer owns **exactly one new file** and must touch **no other file**. They depend on each other only via the **exact signatures** in this spec (an interface contract), never by editing a shared file. All shared-file wiring is done by the integrator (Claude) in a single sequential pass. Each implementer runs in its **own git worktree** (full isolation incl. its own `target/`), branched from a scaffold commit where all module stubs already exist and the crate already compiles — so each can run `cargo test` to self-verify.

| Unit | File (owned) | Implementer | Depends on | Parallel group |
|---|---|---|---|---|
| **A** | `cli/src/cursor_cwd.rs` | cursor-agent #1 | — | Wave 1 |
| **B** | `cli/src/cursor_parse.rs` | cursor-agent #2 | — | Wave 1 |
| **INT** | `agents.rs`, `main.rs`, `adapters/*`, `doctor.rs`, … (shared) | Claude (integrator) | A, B done | After Wave 1 |
| **VERIFY** | (no source) real-behavior tests vs `chorus` binary | Claude | INT done | After INT |
| **D (stretch)** | `cli/src/hermes_reader.rs` + `cli/src/adapters/hermes.rs` | cursor-agent #3 | Cursor fully verified | Wave 2 |
| **INT-H** | enumeration wiring for hermes (shared) | Claude | D done | After Wave 2 |

A and B are fully independent (different files, no shared symbols) → safe to run concurrently. INT is sequential and owned by the integrator because it reuses `agents.rs` privates. Hermes is gated on Cursor success (per the stretch contract).

---

## 6. Unit specs

> **Universal rules for every implementer (composer-2.5):**
> - Edit **only** your assigned file(s). Do **not** modify `main.rs`, `agents.rs`, `Cargo.toml`, or any other file. The module is already declared and the crate already compiles.
> - **No new dependencies.** This crate has **no dev-dependencies** — do **not** use the `tempfile` crate. For tests, create temp dirs with this exact idiom:
>   ```rust
>   fn fixture(name: &str) -> std::path::PathBuf {
>       let dir = std::env::temp_dir().join(format!("chorus_<unit>_{}", name));
>       let _ = std::fs::remove_dir_all(&dir);
>       std::fs::create_dir_all(&dir).unwrap();
>       dir
>   }
>   ```
> - Allowed deps only: `std`, `serde_json` (already a dependency), `anyhow` (already a dependency). No `serde` derive needed; parse with `serde_json::Value`.
> - Keep every public function signature **exactly** as written here (the integrator calls them verbatim).
> - Put unit tests in a `#[cfg(test)] mod tests { ... }` at the bottom of your file.
> - Self-verify before finishing: `cargo test <your_module>::` must pass and `cargo build` must be clean (no warnings about your file).
> - Do not run `git`, do not commit, do not edit files outside your module.

### Unit A — `cli/src/cursor_cwd.rs`

Purpose: resolve the real workspace directory for a Cursor transcript file.

**Exact public API:**
```rust
use std::path::{Path, PathBuf};

/// Walk `tokens` (a path split on '-') as a chain of EXISTING directories under
/// `base`, where a single real directory name may itself span several tokens
/// (because real names can contain '-'). Returns the deepest matched path iff the
/// FULL token list is consumed by existing directories; otherwise None.
/// Backtracking, longest-match-first.
pub(crate) fn walk_existing(base: &Path, tokens: &[&str]) -> Option<PathBuf>;

/// Demangle a Cursor project dir name (e.g. "Users-e059303-sandbox-work-trust-stream-trust-stream-backend")
/// into the real absolute path it maps to, by fs-walking from "/". None if no
/// existing path matches.
pub(crate) fn demangle_project_dir(project_name: &str) -> Option<PathBuf>;

/// Resolve the originating workspace cwd for a transcript file at
/// `<...>/<project>/agent-transcripts/<session>/<session>.jsonl`.
/// Order: (1) <project>/.workspace-trusted -> "workspacePath"; (2) demangle the
/// <project> dir name; (3) None.
pub(crate) fn resolve_cursor_cwd(transcript_path: &Path) -> Option<PathBuf>;
```

**Behavior details:**
- `walk_existing`: if `tokens` is empty → `Some(base.to_path_buf())` if `base.is_dir()` else `None`. Otherwise, for `j` from `tokens.len()` down to `1`: let `name = tokens[..j].join("-")`; if `base.join(&name).is_dir()`, recurse `walk_existing(&base.join(name), &tokens[j..])`; return the first `Some`. Else `None`.
- `demangle_project_dir`: `walk_existing(Path::new("/"), &project_name.split('-').collect::<Vec<_>>())`.
- `resolve_cursor_cwd`: project dir = `transcript_path.parent()?.parent()?.parent()?`. Try `project_dir.join(".workspace-trusted")`: read to string, `serde_json::from_str::<Value>`, `["workspacePath"].as_str()` → `PathBuf`. If that yields a path, return it. Else `demangle_project_dir(project_dir.file_name()?.to_str()?)`.

**Required tests (≥6):**
- `walk_existing` matches a simple subdir chain (tempdir with `a/b`).
- `walk_existing` matches a dashed chain: create `base/trust-stream/trust-stream-backend`; tokens `["trust","stream","trust","stream","backend"]` → returns that path.
- `walk_existing` rejects a non-existent chain (`["nope"]` → None).
- `walk_existing` disambiguates: create only `base/play-foo` (a single dir literally named `play-foo`); tokens `["play","foo"]` → matches `base/play-foo`; but if instead only `base/play/foo` exists, the same tokens match `base/play/foo`. (Two separate tests.)
- `resolve_cursor_cwd` via `.workspace-trusted`: build a tempdir `<proj>/agent-transcripts/<sess>/<sess>.jsonl` and `<proj>/.workspace-trusted` with `{"workspacePath":"/tmp/whatever"}` → returns `/tmp/whatever`.
- `resolve_cursor_cwd` returns None when neither source resolves (no `.workspace-trusted`, project name doesn't exist under `/`).

### Unit B — `cli/src/cursor_parse.rs`

Purpose: parse a Cursor transcript into ordered user/assistant turns.

**Exact public API:**
```rust
use std::path::Path;
use serde_json::Value;

/// One conversation turn from a Cursor transcript.
pub(crate) struct CursorTurn {
    pub role: String,
    pub text: String,
}

/// Flatten a Cursor transcript line's `message` value into plain text.
/// - object with "content": [ {type:"text", text}, ... ] -> concat the "text" of
///   segments whose "type" == "text", in order (ignore tool_use / other types).
/// - object with "content": "<string>" -> that string.
/// - string -> the string as-is.
/// - anything else -> "".
pub(crate) fn flatten_cursor_message(message: &Value) -> String;

/// Read a Cursor transcript (.jsonl) into turns. Skips: non-JSON lines, lines
/// whose role is not "user"/"assistant", and turns whose flattened text is empty
/// (after trim). Preserves order.
pub(crate) fn read_cursor_turns(path: &Path) -> Vec<CursorTurn>;
```

**Behavior details:**
- `read_cursor_turns`: read file to string (on read error → return empty vec); for each non-empty line, `serde_json::from_str::<Value>(line)` (skip on error); `role = v["role"].as_str()`; skip unless `user`/`assistant`; `text = flatten_cursor_message(&v["message"]).trim()`; skip if empty; push `CursorTurn`.
- Do not redact here (the integrator applies `redact_sensitive_text` downstream).

**Required tests (≥5):**
- `flatten_cursor_message` on the real object+array form (text + tool_use segments) → only the text segments, concatenated.
- `flatten_cursor_message` on `{"content":"hello"}` → `"hello"`.
- `flatten_cursor_message` on `Value::String("raw")` → `"raw"`.
- `flatten_cursor_message` on a number/null → `""`.
- `read_cursor_turns` on a temp `.jsonl` containing: a user line (object/array), an assistant line (text+tool_use), a `"role":"tool"` line, an invalid-JSON line, and an assistant line with empty text → returns exactly `[user, assistant]` with correct text, tool/invalid/empty skipped.

### Unit D — Hermes scaffold (Wave 2, stretch, **not behavior-tested**)

Files: `cli/src/hermes_reader.rs` and `cli/src/adapters/hermes.rs`.

Because Hermes's real format is unknown, scaffold a **provisional** JSONL reader:
- Base dir: env `CHORUS_HERMES_DATA_DIR`, else `~/.hermes/sessions` (PROVISIONAL — document as TBD).
- Provisional line shape (claude-like): `{"role":"user"|"assistant","content":"<string>","cwd":"<optional>"}`.
- `read`/`list`/`search` must return cleanly (empty list / a clear "No Hermes session found." error) when the dir is absent.
- `agents::Session.agent` is `&'static str` → use `"hermes"`.

`adapters/hermes.rs` implements `AgentAdapter` exactly like `adapters/cursor.rs`, delegating to `hermes_reader` functions. Unit tests: parser on a synthetic fixture + "missing dir yields empty list". **No real Hermes data exists; do not attempt to run Hermes.**

---

## 7. Integration plan (Claude — sequential, after Wave 1)

All edits to **shared files**, done once:

1. **`main.rs`**: `mod cursor_cwd;` `mod cursor_parse;` (stubs already added in scaffold — keep). For hermes (Wave 2): `mod hermes_reader;`, add `Hermes` to `enum AgentType`, and to the two display maps (lines ~670, ~1455).
2. **`agents.rs` — cursor reader rewrite** (keep functions here; reuse privates):
   - `cursor_base_dir()` → default `~/.cursor/projects`; keep `CHORUS_CURSOR_DATA_DIR`/`BRIDGE_CURSOR_DATA_DIR` overrides (now meaning "Cursor projects root").
   - `read_cursor_session_with_options(id, cwd, last_n, opts)`: enumerate `<base>/*/agent-transcripts/*/*.jsonl`; if `id`, filter by path-substring; for cwd scoping (no id), use `find_latest_by_cwd(&files, &expected_cwd, |p| cursor_cwd::resolve_cursor_cwd(p))` — note `find_latest_by_cwd` takes a `fn` pointer; if a closure won't coerce, add a free `fn get_cursor_session_cwd(p: &Path) -> Option<PathBuf> { cursor_cwd::resolve_cursor_cwd(p) }`; build turns via `cursor_parse::read_cursor_turns` → map into `ConversationTurn`; reuse the existing `include_user`/`last_n` rendering; `redact_sensitive_text`; set `cwd: resolve_cursor_cwd(target).map(|p| p.display().to_string())`; **drop `cursor_warning()`**.
   - `list_cursor_sessions` / `search_cursor_sessions`: enumerate projects; derive cwd via `resolve_cursor_cwd`; filter with `cwd_matches_project` against normalized `--cwd`; put the **real** cwd in the `"cwd"` field.
   - Remove dead SQLite scanning, `cursor_warning`, and (optionally) `detect_cursor_vscdb_fallback_hint` or demote it.
3. **`adapters/cursor.rs`**: unchanged (already delegates to `agents::*`).
4. **Hermes wiring (Wave 2)**: add `"hermes"` to `messaging.rs VALID_AGENTS`, `timeline.rs ALL_AGENTS`, `checkpoint.rs ALL_AGENTS`, `doctor.rs ALL_AGENTS`, `report.rs` valid match, `adapters/mod.rs get_adapter`, and add `HERMES_ROASTS` + match arm in `agents.rs`. (doctor & timeline parity then come for free — they iterate `ALL_AGENTS` and call the adapter.)

---

## 8. Verification plan (Claude — real behavior, after INT)

1. `cargo build` clean; `cargo test` (incl. A's and B's module tests) green.
2. **Unset the bridge env** for these tests (`unset CHORUS_CURSOR_DATA_DIR`) so the binary reads `~/.cursor/projects`. Build the binary (`cargo build --release` or use `target/.../chorus`).
3. Real-behavior matrix against the actual `~/.cursor/projects`:
   - `chorus read --agent cursor --cwd ~/sandbox/work --json` → returns the **work** Cursor session content; `cwd` field populated; **no** "no scoping" warning; metadata/marker absent.
   - same with `--cwd ~/sandbox/play` → returns a **play** session (different from work) → proves real `--cwd` scoping (the thing the bridge couldn't do).
   - `chorus list --agent cursor --cwd ~/sandbox/work --json` → only work-cwd sessions; `cwd` is the real path.
   - `chorus doctor --cwd ~/sandbox/work` → `sessions_cursor PASS`.
   - `chorus timeline --cwd ~/sandbox/work --json` → cursor appears.
   - `chorus read --agent cursor --include-user --json` and `--tool-calls` behave like other agents.
   - redaction: plant a fake AWS key (`AKIAIOSFODNN7EXAMPLE`) in a throwaway transcript under a temp projects root, point `CHORUS_CURSOR_DATA_DIR` at it, confirm `read` masks it.
4. Parity check: same command surface works for cursor as for `--agent codex`/`--agent claude`.
5. Hermes (Wave 2): `cargo build` clean; `chorus doctor` lists `sessions_hermes` (WARN "no sessions" is the pass condition since none exist); `get_adapter("hermes")` resolves. **No behavior test.**
6. If all green: decommission the bridge — `cursor-chorus-bridge --clean`, remove the `~/.zshenv` routing + `chorus` wrapper, drop `~/.zshrc` note. (Separate confirmed step.)

---

## 9. Execution mechanics

- **Branch:** `feat/native-cursor-hermes-adapter` off current HEAD. Scaffold commit adds stub modules + declarations so the crate compiles before any agent runs.
- **Isolation:** one `git worktree` per implementer (`git worktree add ../acw-<unit> feat/native-cursor-hermes-adapter`), so a `--force` agent can't disturb peers and each gets its own `target/`. Integrator copies back only the owned file(s).
- **Agent invocation (headless, auto-run):**
  ```
  cd <worktree> && cursor-agent --print --force --model composer-2.5 "<verbatim unit prompt>"
  ```
  Run Wave-1 agents concurrently (background). Monitor logs; on completion, `cargo test` in each worktree, then integrate.
- **Cost/trust note:** `cursor-agent` makes **paid calls on the Edelman work Cursor account** and `--force` grants write+shell autonomy in the worktree. Confirm-tier — requires explicit user go-ahead before dispatch.

---

## 10. Risks & rollback

- **Signature drift by a small model** → integration breaks. Mitigation: exact signatures in §6; integrator compiles and fixes/redispatches.
- **`--force` agent edits outside its file** → contained by worktree; integrator copies only the owned file.
- **`find_latest_by_cwd` wants a `fn` not a closure** → add the named `get_cursor_session_cwd` shim (§7.2).
- **cwd unknowable for never-trusted+deleted workspaces** → documented limitation (§3).
- **Rollback:** all work is on a branch; abandon the branch and the bridge still works unchanged. Nothing destructive until the explicit decommission step (§8.6).
```
