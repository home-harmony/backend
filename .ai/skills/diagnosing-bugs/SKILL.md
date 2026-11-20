---
name: diagnosing-bugs
description: >-
  Use when investigating failing tests, Aurora DSQL OCC (40001) retry issues,
  keyset pagination edge cases, Decimal math discrepancies, or unexpected runtime
  behavior in backend/. Enforces a scientific 6-phase loop starting with a
  deterministic failing reproduction command.
---

# Scientific Bug Diagnosis Protocol

Prevent speculative guess-edits when debugging Rust logic, SQLx queries, OCC serialization conflicts, or Axum handlers.

---

## Phase 1: Build a Fast, Deterministic Red Loop (Hard Gate)

Construct **one deterministic command** that reliably reproduces the bug (`cargo test -p <crate> <test_name>` or an isolated Axum `oneshot` / Testcontainers PG test).
- Redact any tokens/env values (`<REDACTED>`).
- **Hard Gate**: Do **not** form hypotheses or edit production code until you execute this command and observe it fail.

---

## Phase 2: Minimize the Reproduction

Strip away unrelated entities, middleware, fields, and setup steps until removing one more factor causes the bug to disappear. Keep the minimal reproduction concise and fast.

---

## Phase 3: Formulate 3–5 Ranked, Falsifiable Hypotheses

Before probing, list **3 to 5 distinct hypotheses** ranked by probability and speed of falsification (common `home-harmony` failure modes to check: DSQL OCC `40001` exhaustion, keyset tuple tie-breaking `(occurred_at, id)` when timestamps match, `Decimal` scale/rounding mismatch, `X-Family-Id` filter omission, or `Role::Child` permission guard).

---

## Phase 4: Probe One Variable at a Time

Test the highest-ranked hypothesis with targeted instrumentation (`[DEBUG-HYP1]` tagged logs or temporary assertions).
- Change **one variable per run**.
- Re-rank remaining hypotheses immediately after each observation.

---

## Phase 5: Permanent Regression Test & Minimal Fix

1. Promote the minimized reproduction into a permanent test at the proper seam (`Layer 1` `domain/tests/`, `Layer 2a` `infrastructure/tests/`, or `Layer 2b` `api/tests/`).
   - *(Note: If the bug violates an `Approved` rule in `backend/specs/`, follow the `SPEC–CODE MISMATCH` rule rather than altering spec assertions.)*
2. Confirm the regression test is **Red** before the fix (if verifying a mutation, confirm via `git diff` that the test genuinely fails when broken).
3. Apply the minimal root-cause fix respecting `rust-patterns` (`nutype`, `Decimal`, SLAP, zero `.unwrap()`).
4. Verify the regression test and full workspace turn **Green**.

---

## Phase 6: Scrub & Verify

1. Verify zero `[DEBUG-...]` logs or temporary scratch files remain (`git status` and `git diff`).
2. Run:
   ```bash
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```
