#!/bin/sh
# Agent Context Freshness Check — advisory pre-push hook
#
# Install: cp this file to .git/hooks/pre-push && chmod +x .git/hooks/pre-push
# Or: add to your pre-push hook chain via pre-commit framework
#
# This hook warns (never blocks) when context-relevant code was modified
# but agent-context artifacts were not updated in the commits being pushed.
#
# Installed by:
#   agent-context init --install-hook .
#   agent-context install-hook .
#
# Prerequisite:
#   .agent-context/tools/check_freshness.sh exists (copied by init)
#
# Customize CONTEXT_RELEVANT_PATHS to match your repo structure.

CONTEXT_RELEVANT_PATHS="app/ src/ lib/ migrations/"

while read local_ref local_oid remote_ref remote_oid; do
  if [ "$local_oid" = "0000000000000000000000000000000000000000" ]; then
    continue  # branch deletion, skip
  fi

  if [ "$remote_oid" = "0000000000000000000000000000000000000000" ]; then
    base_ref="$local_oid"  # new branch
  else
    base_ref="$remote_oid"
  fi

  if ! BASE_REF="$base_ref" CONTEXT_RELEVANT_PATHS="$CONTEXT_RELEVANT_PATHS" \
    sh .agent-context/tools/check_freshness.sh >/dev/null 2>&1; then
    echo ""
    echo "WARNING: agent-context freshness check suggests context-relevant files changed"
    echo "but .agent-context/ was not updated."
    echo "   Consider running: 'update the agent context'"
    echo "   (This is advisory only — push is not blocked.)"
    echo ""
  fi
done

exit 0  # Never block push — advisory only
