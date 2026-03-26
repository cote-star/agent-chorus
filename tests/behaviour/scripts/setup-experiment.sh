#!/usr/bin/env bash
# setup-experiment.sh — agent-chorus stress test
# Creates tmux session with 4 panes (2 agents × 2 conditions)
set -euo pipefail

BARE_REPO="$HOME/sandbox/play/agent-chorus-bare"
STRUCT_REPO="$HOME/sandbox/play/agent-chorus-structured"
SESSION="ac-experiment"

# Verify branches
bare_branch=$(git -C "$BARE_REPO" branch --show-current)
struct_branch=$(git -C "$STRUCT_REPO" branch --show-current)

[[ "$bare_branch" == "test/bare" ]] || { echo "ERROR: bare on '$bare_branch', expected 'test/bare'"; exit 1; }
[[ "$struct_branch" == "test/structured" ]] || { echo "ERROR: structured on '$struct_branch', expected 'test/structured'"; exit 1; }

# Verify structured has context pack, bare does not
[[ -f "$STRUCT_REPO/.agent-context/current/routes.json" ]] || { echo "ERROR: structured missing routes.json"; exit 1; }
[[ ! -f "$BARE_REPO/.agent-context/current/routes.json" ]] || { echo "ERROR: bare has routes.json — should be stripped"; exit 1; }

echo "✓ Branches and content verified"

# Timing
TIMING_DIR="$STRUCT_REPO/tests/behaviour/results/.run_timing"
mkdir -p "$TIMING_DIR"
date +%s > "$TIMING_DIR/session_start.txt"
echo "✓ Session start: $(cat "$TIMING_DIR/session_start.txt")"

# Kill existing
tmux has-session -t "$SESSION" 2>/dev/null && tmux kill-session -t "$SESSION"

# Create 2×2 layout
tmux new-session -d -s "$SESSION" -x 240 -y 60 -c "$BARE_REPO"
tmux split-window -t "$SESSION" -h -c "$STRUCT_REPO"

PANES=( $(tmux list-panes -t "$SESSION" -F '#{pane_id}') )
tmux split-window -t "${PANES[0]}" -v -c "$BARE_REPO"
tmux split-window -t "${PANES[1]}" -v -c "$STRUCT_REPO"

readarray -t PANE_INFO < <(tmux list-panes -t "$SESSION" -F '#{pane_id} #{pane_left} #{pane_top}' | sort -k2,2n -k3,3n)
PANE_IDS=()
for line in "${PANE_INFO[@]}"; do PANE_IDS+=( "$(echo "$line" | awk '{print $1}')" ); done

tmux send-keys -t "${PANE_IDS[0]}" "echo '=== CLAUDE | bare ===' && git branch --show-current" Enter
tmux send-keys -t "${PANE_IDS[1]}" "echo '=== CODEX  | bare ===' && git branch --show-current" Enter
tmux send-keys -t "${PANE_IDS[2]}" "echo '=== CLAUDE | structured ===' && git branch --show-current" Enter
tmux send-keys -t "${PANE_IDS[3]}" "echo '=== CODEX  | structured ===' && git branch --show-current" Enter

echo "${PANE_IDS[1]}" > "$TIMING_DIR/pane_id_codex_bare.txt"
echo "${PANE_IDS[3]}" > "$TIMING_DIR/pane_id_codex_struct.txt"

echo ""
echo "══════════════════════════════════════════════════════════════"
echo "  tmux session '$SESSION' is ready."
echo "  Attach: tmux attach -t $SESSION"
echo ""
echo "  ┌─────────────────────────┬─────────────────────────┐"
echo "  │  CLAUDE  bare           │  CLAUDE  structured     │"
echo "  ├─────────────────────────┼─────────────────────────┤"
echo "  │  CODEX   bare           │  CODEX   structured     │"
echo "  └─────────────────────────┴─────────────────────────┘"
echo ""
echo "  Claude: claude --model claude-opus-4-6"
echo "  > read tests/behaviour/EXPERIMENT.md and follow the protocol exactly"
echo ""
echo "  Codex:"
echo "    for f in $TIMING_DIR/pane_id_codex_*.txt; do"
echo "      tmux send-keys -t \$(cat \$f) 'codex -m gpt-5.4-high \"read tests/behaviour/EXPERIMENT.md and follow the protocol exactly\"' Enter"
echo "    done"
echo "══════════════════════════════════════════════════════════════"
