# Behavior Specs — Operational Guidelines

> Operational rules for [ADR-0019](../../documentation/decisions/0019-declarative-test-specifications.md).
> Template: [`TEMPLATE.md`](./TEMPLATE.md) · Examples: [`examples.md`](./examples.md) · Agent skill: [`spec-driven-dev`](../../documentation/.ai/skills/spec-driven-dev/SKILL.md).

---

## 0. Lifecycle Overview

```text
 ┌────────────┐   ┌──────────────┐   ┌─────────────┐   ┌──────────────┐   ┌───────────┐
 │ A. WRITE   │ → │ B1. REVIEW   │ → │ APPROVE     │ → │ C. GENERATE  │ → │ B2. REVIEW│
 │ spec rules │   │ the spec     │   │ (human only)│   │ tests, blind │   │ the tests │
 └────────────┘   └──────────────┘   └─────────────┘   └──────────────┘   └─────┬─────┘
        ▲                                                                         │
        └──────── red test = SPEC–CODE MISMATCH, human decides who is wrong ◄─────┘
```

| Step | Human | AI |
| :--- | :--- | :--- |
| **A. Write** | Writes or refines rules directly | Drafts rules with `Origin`; **never approves** |
| **B1. Review spec** | Decides *"is this what the system should do?"* | Runs Q1–Q10 pre-review table |
| **Approve** | Sets `Status: Approved` and confirms `Origin` (**human only**) | — |
| **C. Generate** | — | Generates tests in `backend/*/tests/` blindly |
| **B2. Review tests** | Confirms each test faithfully encodes its rule | Checks 1:1 rule $\leftrightarrow$ test mapping |
| **Red test** | Decides whether code or spec is wrong | Halts and reports mismatch; **never weakens assertion** |

---

## 1. Formats, IDs & Statuses

### Choosing the Format & Seam

| Behavior Type | Format | Spec Location (`backend/`) | Test Target (`backend/`) | Seam |
| :--- | :--- | :--- | :--- | :--- |
| State machines, decision/RBAC matrices | **T** (Case Table) | `specs/domain/*.md` | `domain/tests/` | Layer 1 |
| Value-object invariants, financial formulas | **P** (Property) | `specs/domain/*.md` | `domain/tests/` | Layer 1 |
| Multi-step flows, API & auth gates | **U** (Use Case) | `specs/api/*.md` | `api/tests/` or `lambdas/*/tests/` | Layer 2a / 2b |

### Rule IDs
`<PREFIX>-<NN>`: 2–4 uppercase letters, a dash, 2–3 digits (`VO-01`, `FM-12`, `BG-103`).
- Register every prefix once in [`README.md`](./README.md).
- IDs are globally unique and **never reused or renumbered** (removed rules become `Deprecated`).

### Statuses & Automated CI Guardrails

| Status | Meaning | Traceability Gate (`spec_traceability.rs`) |
| :--- | :--- | :--- |
| `Draft` | Proposed or unreviewed | Tests are **forbidden** — citing a `Draft` rule in `tests/*.rs` fails CI |
| `Approved` | Human-accepted source of truth (`Origin` cannot be `observed`, no open questions) | **Mandatory** — requires matching test in `backend/*/tests/*.rs` |
| `Deprecated` | Retired behavior (ID stays reserved) | Tests are **forbidden** — citing a `Deprecated` rule fails CI |

Changing `Given / When / Then / For all / Oracle` of an `Approved` rule resets it to `Draft` for re-approval.

---

## A. Writing Specs (`Status: Draft`)

### A1. Rule Anatomy
- **ID**: `<PREFIX>-<NN>`
- **Status**: Always `Draft` when authored by AI.
- **Origin**:
  - `human` — stated or confirmed directly by the project owner.
  - `adr:NNNN` / `doc:<path>` — derived from an accepted ADR or architecture doc.
  - `observed` — inferred by AI from existing code/tests (CI blocks promoting `observed` rules to `Approved` until human confirms intent by updating `Origin`).
- **Given / When / Then** (or **For all / Then / Oracle / Boundary Examples** for Format P).

### A2. Quality Criteria (`Q1–Q10`)

| # | Criterion | Anti-Pattern to Reject |
| :--- | :--- | :--- |
| `Q1` | **Atomic** — one behavior, one failure reason | Combining creation, event emission, and authorization in one rule |
| `Q2` | **Observable** — `Then` uses public API or HTTP responses only | Asserting private helper calls |
| `Q3` | **Concrete** — exact boundary values, not adjectives | "long name fails" instead of "201 chars fails, 200 succeeds" |
| `Q4` | **Closed vocabulary** — uses `Vocabulary` + exact `DomainError` variants | "returns an error" |
| `Q5` | **Intent over implementation** — survives refactoring | "calls `assert_same_currency()`" |
| `Q6` | **Complete state** — all relevant preconditions stated | Omitting member `Role` or currency |
| `Q7` | **Boundaries explicit** — min, max, empty, zero, negative | Testing only the nominal middle value |
| `Q8` | **Negative cases included** — every guard has a failing rule | Happy paths only |
| `Q9` | **No duplicates or contradictions** — log conflicts in Findings | Two rules contradicting each other |
| `Q10` | **Not redundant with Rust types** — skip what compiler already prevents | "accepts a `CurrencyCode` struct" |

---

## B. Reviewing (`B1` Spec Review & `B2` Test Review)

- **B1 (Pre-Approval AI Table)**: AI outputs `| ID | Severity (blocker/major/minor) | Check # | Finding | Suggested Resolution |` without modifying rules or statuses.
- **B2 (Post-Generation Check)**: Confirm 1:1 rule ID mapping, `Arrange == Given`, single `Act == When`, `Assert == Then`, error matching by enum variant (`matches!`), and independent expected values (never recomputed by calling production formulas).

---

## C. Blind Test Generation & Mismatch Protocol

### C1. Context Rules for Test Generation
- **Allowed**: `Approved` rules + `Vocabulary` in `backend/specs/**`, public signatures & doc comments (`pub` items), `backend` test helpers/fakes.
- **Forbidden**: Reading function implementation bodies or existing inline `#[cfg(test)]` blocks of the unit under test. (When running in a session without body-isolation, explicitly note that blindness was constrained so the human reviews carefully.)

### C2. Conventions
- Place spec tests in `backend/<crate>/tests/<prefix_lowercase>_<topic>.rs`.
- Include the rule ID in every test function name (`fn fm_01_owner_can_access_debt()`) or table case struct (`id: "FM-01"`).
- Use `.expect("reason")` only in test setup (per `backend/clippy.toml`).
- Verify with `cargo test -p domain --test spec_traceability` and `cargo clippy --workspace --all-targets -- -D warnings`.

### C3. Red-Test Protocol (`SPEC–CODE MISMATCH`)
If a generated test faithfully implements an `Approved` rule and fails against production code:
1. **Do NOT modify the assertion, the spec rule, or the production code silently.**
2. Leave the failing test in place and emit a `SPEC–CODE MISMATCH DETECTED` report naming the Rule ID, Spec file, Expected outcome, Actual outcome, and Failing test.
3. Wait for human resolution (fix production code or revise spec back to `Draft`).
