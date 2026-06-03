# Acceptance Tests

These tests were run during pack creation to verify the agent context actually helps agents on real tasks. Each test compares a pack-only answer against grep-verified ground truth.

## Test 1: Lookup

**Question:** REPLACE: a specific lookup question targeting a lightly-covered subsystem

**Pack-only answer:** REPLACE: what the pack says

**Grep verification:** REPLACE: what the actual code shows

| Metric | Value |
|---|---|
| Pack pointed to correct files? | REPLACE: yes/no |
| Files opened to verify | REPLACE: count |

---

## Test 2: Impact Analysis

**Question:** REPLACE: a change that touches ≥2 subsystems

**Pack-only answer (files that must change):** REPLACE: file list from pack

**Grep verification (files that actually must change):** REPLACE: file list from grep

| Metric | Value |
|---|---|
| Files identified by pack | REPLACE: count |
| Files found by grep | REPLACE: count |
| Coverage ratio | REPLACE: pack/grep as percentage |
| False positives | REPLACE: count and list |
| False negatives | REPLACE: count and list |
| Pass (≥80%)? | REPLACE: yes/no |

---

## Test 3: Cross-Cutting Impact

**Question:** REPLACE: a change spanning 3+ subsystems (auth, config, storage, etc.)

**Pack-only answer:** REPLACE: files and patterns from pack

**Grep verification:** REPLACE: full blast radius from grep

| Metric | Value |
|---|---|
| Core files identified by pack | REPLACE: count/total |
| Downstream files findable via pack's grep guidance | REPLACE: count |
| Files grep found that pack missed entirely | REPLACE: count and list |
| Coverage ratio (core files) | REPLACE: percentage |
| Pass (≥80% core)? | REPLACE: yes/no |

---

## Test 4: Diagnosis

**Question:** REPLACE: a runtime failure involving an external integration

**Pack-only diagnosis plan:** REPLACE: investigation steps from pack

**Source verification:** REPLACE: what the actual trace revealed

| Metric | Value |
|---|---|
| Pack pointed to correct subsystem? | REPLACE: yes/no |
| Pack avoided dead ends? | REPLACE: yes/no |
| Files opened to verify | REPLACE: count |
| Additional files needed beyond pack guidance | REPLACE: count and list |

---

## Summary

| Test | Category | Pass? |
|---|---|---|
| 1 | Lookup | REPLACE |
| 2 | Impact analysis | REPLACE |
| 3 | Cross-cutting impact | REPLACE |
| 4 | Diagnosis | REPLACE |

**Overall:** REPLACE: all pass / N failures requiring pack improvements

**Iterations:** REPLACE: how many pack improvements were made based on test failures before all tests passed
