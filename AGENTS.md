# Agent Instructions For This Repo

> **Naming convention**: Use `bridge context-pack ...` commands. Legacy npm
> wrappers (`npm run context-pack:*`) are still available in this repo.

## End-to-End Understanding Shortcut
When asked to understand this repository end-to-end:
1. Read `.agent-context/current/00_START_HERE.md` first.
2. Use `.agent-context/current/manifest.json` + `20_CODE_MAP.md` to target only relevant source files.
3. Open additional files only when the current task requires deeper proof.

## If Context Pack Is Missing or Stale
Run:

```bash
bridge context-pack init
# ...fill details...
bridge context-pack seal
```

## Main Push Context Sync
Install hook once:

```bash
bridge context-pack install-hooks
```

The pre-push hook prints an advisory warning when a push targets `main` and changes context-relevant files. It never auto-builds or blocks the push.
