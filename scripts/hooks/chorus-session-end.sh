#!/bin/bash
# chorus-session-end.sh
#
# Claude Code SessionEnd hook — fires when the CLI session terminates,
# whether via clean exit, crash, or window close.
#
# INSTALL: Add to ~/.claude/settings.json (global) or .claude/settings.json (project):
#
#   "hooks": {
#     "SessionEnd": [{
#       "hooks": [{
#         "type": "command",
#         "command": "bash /path/to/chorus-session-end.sh",
#         "timeout": 10
#       }]
#     }]
#   }
#
# PURPOSE: Ensures other agents always receive Claude's git state on exit —
# even when /conclude is not run (crash, interruption, context limit hit).
# The message is lightweight (branch + uncommitted count + last commit) and
# complements, rather than replaces, the detailed handoff from /conclude.
#
# SCOPE: Only sends if the current project has .agent-chorus/ set up.
# Safe to install globally — no-ops on projects without Chorus.

set -e

CWD="${CLAUDE_PROJECT_DIR:-$(pwd)}"

# Guard: only run if this project uses Chorus
if [ ! -d "$CWD/.agent-chorus" ]; then
  exit 0
fi

cd "$CWD"

# Collect current git state
BRANCH=$(git branch --show-current 2>/dev/null || echo "unknown")
UNCOMMITTED=$(git status --short 2>/dev/null | wc -l | tr -d ' ')
LAST_COMMIT=$(git log -1 --format="%h %s" 2>/dev/null || echo "none")

MSG="Claude session ended. Branch: ${BRANCH} | Uncommitted files: ${UNCOMMITTED} | Last commit: ${LAST_COMMIT} | Check STATUS.md and session-logs/ for full context."

# Send to each known agent — ignore failures (chorus may not be in PATH in all envs)
chorus send --from claude --to codex  --message "$MSG" --cwd "$CWD" 2>/dev/null || true
chorus send --from claude --to gemini --message "$MSG" --cwd "$CWD" 2>/dev/null || true

exit 0
