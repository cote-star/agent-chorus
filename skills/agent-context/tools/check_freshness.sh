#!/bin/sh
# Check whether context-relevant files changed without a corresponding pack update.

set -eu

BASE_REF="${BASE_REF:-origin/main}"
CONTEXT_RELEVANT_PATHS="${CONTEXT_RELEVANT_PATHS:-app/ src/ lib/ migrations/}"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --base-ref)
      BASE_REF="$2"
      shift 2
      ;;
    --paths)
      CONTEXT_RELEVANT_PATHS="$2"
      shift 2
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

CODE_CHANGED=$(git diff --name-only "$BASE_REF"...HEAD -- $CONTEXT_RELEVANT_PATHS | wc -l | tr -d ' ')
PACK_CHANGED=$(git diff --name-only "$BASE_REF"...HEAD -- .agent-context/ | wc -l | tr -d ' ')

if [ "$CODE_CHANGED" -gt 0 ] && [ "$PACK_CHANGED" -eq 0 ]; then
  echo "ERROR: $CODE_CHANGED context-relevant files changed but .agent-context/ was not updated"
  exit 1
fi

echo "OK: freshness check passed"
