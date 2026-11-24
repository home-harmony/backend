# AGENTS.md — HomeHarmony (`backend/` Rust & Aurora DSQL Entry Point)

> Entry point for AI agents working in `backend/`.
> Also read cross-repo governance and domain rules in [`../documentation/AGENTS.md`](../documentation/AGENTS.md).

---

## 1. Reading Order

1. [`README.md`](./README.md) — workspace crates & developer commands
2. [`.ai/rules/backend-rust.md`](./.ai/rules/backend-rust.md) — hexagonal crates, Deep Modules, RPITIT, `nutype`, `Decimal`, SLAP/Newspaper layout
3. [`.ai/rules/aurora-dsql.md`](./.ai/rules/aurora-dsql.md) — UUID v4/v7, 1-DDL-per-file, `CREATE INDEX ASYNC`, OCC retry, `BEGIN READ ONLY`, keyset
4. [`../documentation/domain/ubiquitous-language.md`](../documentation/domain/ubiquitous-language.md) — canonical domain terms, formulas, and 10 invariants
5. [`specs/README.md`](./specs/README.md) — self-contained behavior specifications governing tests

---

## 2. Rules & Skills Index

### A. Modular Rules (Always Enforced)
- **Common Workspace Rules** (auto-included by [`../documentation/AGENTS.md`](../documentation/AGENTS.md)): [`../documentation/.ai/rules/common.md`](../documentation/.ai/rules/common.md)
@[Backend Rust Rules](./.ai/rules/backend-rust.md)
@[Aurora DSQL Rules](./.ai/rules/aurora-dsql.md)

### B. Skills (Auto-Discovered via `.agents/skills.json`)

| Scope | Task | Skill (`SKILL.md`) |
| :--- | :--- | :--- |
| **Backend** | Design or refactor clean Rust modules (`nutype`, RPITIT, SLAP, Newspaper layout) | [`.ai/skills/rust-patterns/SKILL.md`](./.ai/skills/rust-patterns/SKILL.md) |
| **Backend** | Execute Red-Green-Refactor TDD & blind spec tests across Layer 1 / 2a / 2b seams | [`.ai/skills/rust-testing/SKILL.md`](./.ai/skills/rust-testing/SKILL.md) |
| **Backend** | Author Aurora DSQL migrations (`1 DDL/file`), async indexes, OCC/keyset SQLx queries | [`.ai/skills/dsql-migrations/SKILL.md`](./.ai/skills/dsql-migrations/SKILL.md) |
| **Backend** | Reproduce and isolate bugs scientifically (*"no red test, no theories"*) | [`.ai/skills/diagnosing-bugs/SKILL.md`](./.ai/skills/diagnosing-bugs/SKILL.md) |
| **Cross-Repo** | Align domain glossary, research primary sources, or propose an ADR | [`../documentation/.ai/skills/domain-and-adr/SKILL.md`](../documentation/.ai/skills/domain-and-adr/SKILL.md) |
| **Cross-Repo** | Draft/review `Draft` specs, slice epics vertically, or handle `SPEC–CODE MISMATCH` | [`../documentation/.ai/skills/spec-driven-dev/SKILL.md`](../documentation/.ai/skills/spec-driven-dev/SKILL.md) |
| **Cross-Repo** | Post-coding 2-axis self-review (Standards vs. Spec) & One-Way Door PR check | [`../documentation/.ai/skills/code-review/SKILL.md`](../documentation/.ai/skills/code-review/SKILL.md) |

---

## 3. Mandatory Verification Gates

Before finishing any code change:
```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
- `cargo test --workspace` runs `domain --test spec_traceability`, enforcing the automated specification guardrails documented in [`specs/README.md`](./specs/README.md).
- Run [`code-review`](../documentation/.ai/skills/code-review/SKILL.md) after every coding change.
- Put temporary scratch notes in `.ai/local/` (gitignored).
