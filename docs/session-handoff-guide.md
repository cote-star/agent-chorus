# Session Handoff Guide

A working session never exists in isolation. Another agent — Codex, Claude,
Gemini, or Cursor — may pick up the same repo an hour later, or tomorrow, or
on another machine. The Session Handoff Protocol is how Chorus keeps that
pickup lossless.

This guide is organised around the **subcommand** surface first. Hooks,
scripts, and provider-specific wiring are described as consumers of that
surface, not as a substitute for it.

## Primitives

| Subcommand | Role |
| :--- | :--- |
| `chorus messages --agent <self>` | Read the inbox that other agents left for you |
| `chorus messages --agent <self> --clear` | Read and drain the inbox after acting on it |
| `chorus send --from <self> --to <other>` | Leave a targeted note for one specific agent |
| `chorus checkpoint --from <self>` | Broadcast a lightweight state snapshot to every other agent |

All four are real CLI surface. `checkpoint` is the newest addition and is
safe to call unconditionally: when `.agent-chorus/` is absent it exits
silently without writing anything.

## Scenario 1 — Clean handoff

The default case. You open a session, do work, close it cleanly.

```bash
# At standup (first thing after opening the session)
chorus messages --agent claude --clear

# ...work happens here...

# At conclude (before ending the session)
chorus send --from claude --to codex --message "payment refactor done; types still TODO"
chorus checkpoint --from claude
```

Use `send` when you have a specific, addressed note for one agent. Use
`checkpoint` when the state is general — "I'm out, here's what I was on" —
and you want every other agent to see it.

You can call both in the same conclude block. They write to different
recipients; they do not conflict.

## Scenario 2 — Interrupted handoff (Claude Code)

When Claude Code exits mid-task — crash, window close, OS restart — there is
no opportunity to type a conclude command. The Claude Code harness does
fire a `SessionEnd` hook before it tears down, and Chorus ships a wrapper
for exactly this case.

Install the hook by editing `~/.claude/settings.json`:

```json
{
  "hooks": {
    "SessionEnd": [{
      "hooks": [
        {
          "type": "command",
          "command": "bash /absolute/path/to/chorus-session-end.sh",
          "timeout": 10
        }
      ]
    }]
  }
}
```

The wrapper script lives at `scripts/hooks/chorus-session-end.sh` in this
repo. It is intentionally thin — it delegates to `chorus checkpoint --from
claude` so that the real logic lives in one place (the subcommand) and the
hook is just plumbing. It canonicalises `CLAUDE_PROJECT_DIR` with
`realpath`, guards on `.agent-chorus/` presence, and backgrounds the
dispatch so a stuck Chorus invocation cannot pin your CLI exit.

Because the script guards on `.agent-chorus/`, you can install it globally
and it will no-op on projects that do not use Chorus. There is no per-repo
install step.

## Scenario 3 — Mid-task checkpoint (Codex, Gemini, Cursor)

Codex, Gemini, and Cursor do not expose a `SessionEnd` hook equivalent. For
those agents, the pattern is different: **call `chorus checkpoint`
yourself at logical break points**.

Natural break points include:

- Finishing a phase of a multi-phase task (e.g. "design done, implementation
  next")
- Before stepping away — lunch, end of day, context switch
- After landing a change that another agent might want to build on

```bash
# From inside a Codex session
chorus checkpoint --from codex --message "auth middleware refactor landed; tests still red"

# From Gemini, with automatic state capture (no --message)
chorus checkpoint --from gemini

# From Cursor, before closing the editor
chorus checkpoint --from cursor
```

When `--message` is omitted, `checkpoint` composes a short message from the
current git state: branch name, uncommitted-file count, and the last
commit's short hash and subject. When `--message` is supplied, that text is
used verbatim.

The result in both cases is a JSONL line appended to each of the other
three agents' message inboxes under `.agent-chorus/messages/`.

## Scenario 4 — Gemini protobuf fallback

Gemini's CLI recently started writing session state as protobuf (`.pb`) in
`~/.gemini/<profile>/conversations/` rather than JSONL in `~/.gemini/tmp/`.
Chorus reads the JSONL form; it does not yet parse the protobuf form.

If `chorus read --agent gemini` returns `NOT_FOUND` and the new `.pb`
layout is the reason, Chorus will say so in the error message. You then
have three options, in order of preference:

### Option A — point Chorus at a JSONL export directly

If you have a JSONL export from a prior Gemini session, use `--chats-dir`
to point Chorus at it:

```bash
chorus read --agent gemini --chats-dir /path/to/jsonl-export --cwd .
```

`--chats-dir` bypasses the default `~/.gemini/tmp/` discovery and scans
only the directory you give it. Use this whenever you have a known-good
JSONL source.

### Option B — override via the environment variable

For long-running shells, set `CHORUS_GEMINI_TMP_DIR` to a directory
containing a JSONL session tree laid out as `<hash>/chats/session-*.json`:

```bash
export CHORUS_GEMINI_TMP_DIR=/path/to/jsonl-root
chorus read --agent gemini
```

This is the same discovery code path as the default, just rooted
elsewhere.

### Option C — write a JSONL stub by hand

As a last resort for a workstation where Gemini has already moved fully to
`.pb` and no JSONL tree exists, you can stand up a minimal fake tree that
satisfies Chorus's schema. Compute the SHA-256 of the project path
(Chorus uses this to scope per-cwd):

```bash
CWD_HASH=$(printf '%s' "$PWD" | shasum -a 256 | awk '{print $1}')
mkdir -p ~/.gemini/tmp/"$CWD_HASH"/chats
cat > ~/.gemini/tmp/"$CWD_HASH"/chats/session-stub.json <<'EOF'
{
  "sessionId": "session-stub",
  "messages": [
    { "type": "user", "content": "(placeholder — Gemini session in .pb)" },
    { "type": "gemini", "content": "(placeholder — use --chats-dir to point at real data)" }
  ]
}
EOF
```

`chorus read --agent gemini --cwd .` will now succeed and return the stub
content. This unblocks the read path for demos and smoke tests; it is not
a replacement for real session data. Full `.pb` parsing is tracked as a
separate piece of work.

## How the pieces fit

The protocol stacks in three layers: subcommands (`messages`, `send`,
`checkpoint`) are the only surface you ever call; per-agent rituals
(documented in `CLAUDE.md`, `AGENTS.md`, `GEMINI.md`) tell each agent
when to invoke them; and the Claude Code `SessionEnd` hook catches the
interruption case for that one provider. When adding new agents, mirror
this order — make the subcommand path work first, then wire rituals
around it, then script interruption only if the agent exposes a hook.

## Troubleshooting

| Symptom | Likely cause | Action |
| :--- | :--- | :--- |
| `chorus checkpoint` exits 0 silently | No `.agent-chorus/` directory in cwd | Run `chorus setup` first |
| `chorus messages --agent claude` returns `[]` | Inbox was cleared by prior read | Omit `--clear` if you want to re-read |
| Hook installed but inbox never fills | `CLAUDE_PROJECT_DIR` resolved outside the repo | Check the hook canonicalises via `realpath` |
| `chorus read --agent gemini` returns `NOT_FOUND` with "protobuf" in the message | Gemini switched to `.pb` format | See Scenario 4 above |

## See also

- `docs/CLI_REFERENCE.md` — full flag reference for every subcommand
- `CLAUDE.md`, `AGENTS.md`, `GEMINI.md` — per-provider ritual documentation
- `scripts/hooks/chorus-session-end.sh` — the thin wrapper script for
  Claude Code
