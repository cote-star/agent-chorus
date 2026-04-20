#!/usr/bin/env bash
# scripts/hooks/chorus-session-end.sh
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
# PURPOSE: Call `chorus checkpoint --from claude` on session end so other
# agents always receive a lightweight state broadcast (branch, uncommitted
# count, last commit) even when an interactive `/conclude` was not run.
#
# SAFETY: The `.agent-chorus/` guard in `chorus checkpoint` means this is
# safe to install globally; it no-ops on projects without chorus wiring.

set -euo pipefail

# Canonicalize to prevent env-var-based path traversal. Fallback to cwd.
CWD="$(realpath "${CLAUDE_PROJECT_DIR:-$PWD}" 2>/dev/null || printf '%s' "$PWD")"

# Cheap early exit when there's nothing to do.
[ -d "$CWD/.agent-chorus" ] || exit 0

# Background so a hanging chorus process doesn't pin the CLI exit past the
# settings.json timeout. `disown` detaches from the parent's job table.
(
  cd "$CWD" || exit 0
  chorus checkpoint --from claude 2>/dev/null || true
) &
disown 2>/dev/null || true

exit 0
