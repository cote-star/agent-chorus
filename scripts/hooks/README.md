# scripts/hooks

Claude Code and agent-system hooks shipped alongside chorus. Each script
is designed to be wired into a harness (e.g. Claude Code `settings.json`)
so chorus state stays in sync with interactive agent sessions.

## chorus-session-end.sh

Claude Code `SessionEnd` hook. Calls `chorus checkpoint --from claude`
when the CLI session terminates (clean exit, crash, or window close) so
other agents always receive a lightweight state broadcast (current
branch, uncommitted-file count, last commit) even when the user never
ran `/conclude`.

### Install

Add to `~/.claude/settings.json` (global) or `.claude/settings.json`
(per-project):

```json
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

Replace `/path/to/` with the absolute path to this file after cloning
the repo (e.g. `~/code/agent-chorus/scripts/hooks/chorus-session-end.sh`).

### Security

The script is safe to install globally because of two guards:

- **`.agent-chorus/` guard** — the script exits immediately when the
  target project has no `.agent-chorus/` directory, and `chorus
  checkpoint` itself no-ops on non-chorus projects.
- **`realpath` canonicalization** — `CLAUDE_PROJECT_DIR` is canonicalized
  before use, preventing env-var-based path traversal. Unset or invalid
  values fall back to the current working directory.

The actual checkpoint runs in a backgrounded subshell with `disown`, so
a hanging `chorus` invocation cannot pin the Claude Code exit past the
`settings.json` timeout.

### Testing

Smoke-test against a throwaway fixture:

```bash
export CLAUDE_PROJECT_DIR=/tmp/fixture
mkdir -p /tmp/fixture/.agent-chorus
bash scripts/hooks/chorus-session-end.sh
```

With no `.agent-chorus/` directory, the script should exit 0 silently.
With the directory present and `chorus` on `PATH`, a checkpoint record
should be written to `/tmp/fixture/.agent-chorus/`.
