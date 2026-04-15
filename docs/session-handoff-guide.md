# Session Handoff Guide

Chorus can read another agent's session history, but only if that session was cleanly concluded. This guide covers three patterns that together give reliable handoffs even when sessions are interrupted.

---

## 1. Clean Handoff — `chorus send` at conclude, `chorus messages` at standup

The simplest protocol: agents leave a message when they stop, read messages when they start.

**At session end (add to your `/conclude` workflow):**

```bash
# Send to each agent you work alongside
chorus send --from claude --to codex  --message "Session ended. Open threads: [list]. Next focus: [one line]." --cwd <project-path>
chorus send --from claude --to gemini --message "Session ended. Open threads: [list]. Next focus: [one line]." --cwd <project-path>
```

**At session start (first action in `/standup`):**

```bash
chorus messages --agent claude --cwd <project-path> --clear --json
```

`--clear` removes the messages after reading so the queue doesn't grow stale.

> **Note:** `chorus send` is point-to-point — there is no `--to all` broadcast. Send separately to each recipient.

---

## 2. Interrupted Handoff — `SessionEnd` hook (Claude Code)

When a session is interrupted before `/conclude` runs (crash, context limit, closed window), no message is sent. The `SessionEnd` hook fires on every Claude Code exit and fills the gap automatically.

**Install the hook** (see `scripts/hooks/chorus-session-end.sh`):

```json
// ~/.claude/settings.json  (global — fires for any project with .agent-chorus/)
{
  "hooks": {
    "SessionEnd": [{
      "hooks": [{
        "type": "command",
        "command": "bash /path/to/chorus-session-end.sh",
        "timeout": 10
      }]
    }]
  }
}
```

The hook sends current git state (branch, uncommitted file count, last commit hash) to all known agents. It is a safety net, not a replacement for the full `/conclude` handoff — both can fire without conflict.

> **`Stop` vs `SessionEnd`:** Use `SessionEnd`, not `Stop`. `Stop` fires after every response turn; `SessionEnd` fires exactly once when the process exits.

---

## 3. Mid-Task Checkpoint — for agents without a hook system

Codex and Gemini do not have an equivalent to Claude Code's `SessionEnd` hook. For those agents, use a shared checkpoint file written at the start of each significant task block:

```bash
cat > ".agent-chorus/CHECKPOINT.md" << 'EOF'
# Agent Checkpoint
**Agent:** codex
**Timestamp:** 2026-04-05T10:00:00Z
**Branch:** feature/my-feature
**Uncommitted files:** 3
**Current task:** Refactoring payment service to use new API
**Files being modified:** src/services/payment.ts, src/types/payment.ts
**Status:** in-progress — do not overwrite without reading this
EOF
```

Any agent reading this file during standup (no chorus command needed — just `cat`) knows the last known in-progress state, even after an interruption.

**Add to standup fallback:**

```bash
# If chorus messages --agent <name> returns empty and this is a recovery session:
cat ".agent-chorus/CHECKPOINT.md" 2>/dev/null || echo "No checkpoint."
```

---

## 4. Gemini / Protobuf Limitation

`chorus read --agent gemini` may return `NOT_FOUND` even when Gemini has active sessions. This happens when Gemini CLI stores sessions as protobuf (`.pb`) files rather than JSONL.

**Affected path:** `~/.gemini/<profile>/conversations/*.pb`  
**Chorus expects:** `~/.gemini/tmp/*.jsonl`

**Workaround A — JSONL stub at conclude:**

Instruct Gemini to write a discoverable stub at session end:

```bash
mkdir -p ~/.gemini/tmp && \
echo "{\"agent\":\"gemini\",\"session\":\"$(date +%Y-%m-%dT%H:%M:%S)\",\"cwd\":\"$(pwd)\",\"content\":\"Session concluded. See session-logs/ for full log.\"}" \
  >> ~/.gemini/tmp/$(date +%Y-%m-%d-%H-%M).jsonl
```

**Workaround B — session-log fallback:**

If `chorus read --agent gemini` fails, read the most recently modified session log from whatever mirror path your project uses (e.g. `~/.agents/memory/<project>/sessions/`).

---

## Permissions (Claude Code)

If Claude Code prompts for permission on chorus commands, add them to `.claude/settings.local.json`:

```json
{
  "permissions": {
    "allow": [
      "Bash(chorus send:*)",
      "Bash(chorus messages:*)",
      "Bash(chorus search:*)",
      "Bash(chorus compare:*)"
    ]
  }
}
```
