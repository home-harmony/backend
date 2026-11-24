---
name: rust-testing
description: >-
  Use when writing, updating, or running Rust tests across backend/ crates.
  Guides Red-Green-Refactor TDD at Hexagonal Seams (Layer 1 domain, Layer 2a
  Testcontainers PG, Layer 2b Axum oneshot + fakes) and blind spec-backed test
  generation for backend/specs/.
---

# Rust TDD & Spec-Backed Testing

Unifies incremental Red-Green-Refactor TDD with `home-harmony`'s 4-layer testing pyramid (`../documentation/testing/testing-strategy.md`) and spec traceability (`ADR-0019`).

---

## 1. Confirm Hexagonal Seams Before Writing Tests

Before authoring tests for a feature or slice, explicitly confirm the target seam(s) with the user:

| Layer | Location | Seam Under Test | Dependencies |
| :--- | :--- | :--- | :--- |
| **Layer 1 (Pure Domain)** | `domain/tests/<prefix>_*.rs` | Public aggregates, `Money`/`nutype` VOs, `RepaymentPlanCalculator` | Zero I/O, fast (`< 1s`) |
| **Layer 2a (DB Adapter)** | `infrastructure/tests/*.rs` | Repository ports (`RPITIT`) against PostgreSQL 16 (`testcontainers`) | `cargo test -p infrastructure` runs offline unit + `migration_invariants`; Docker PG tests use `#[ignore]` (`-- --ignored`) |
| **Layer 2b (API / Handler)** | `api/tests/*.rs` or `lambdas/*/tests/*.rs` | Axum router via `tower::ServiceExt::oneshot()` + `SpyPublisher` / fakes | In-process HTTP + RBAC |
| **Internal Helpers** | `<crate>/src/**/#[cfg(test)]` | Private algorithms with no public surface | Same module only |

---

## 2. Two Workflows: Blind Spec Tests vs. Interactive TDD

### A. Spec-Backed Conformance Tests (from `Approved` rules in `backend/specs/`)
Follow the blind-generation, `Q1–Q10`, and `SPEC–CODE MISMATCH` governance protocol in [`GUIDELINES.md`](../../../specs/GUIDELINES.md) and `spec-driven-dev`. Rust integration conventions:
1. **Extract Public Signatures Blindly**: Use `cargo doc --no-deps` or a read-only signature-extraction subagent so implementation bodies and inline `#[cfg(test)]` blocks remain unread.
2. **Place in Integration Tests**: Write `<crate>/tests/<prefix_lowercase>_<topic>.rs` with 1:1 rule ID citations (`"FM-01"` or `#[test] fn vo_01_...`) for `Approved` rules only (enforced by [`spec_traceability.rs`](../../../domain/tests/spec_traceability.rs)).

### B. Interactive Red-Green-Refactor (New Features)
Work **one vertical behavior slice at a time** (never horizontal "write all tests first, then all code"):
1. **Red**: Write **one** failing test at the agreed seam asserting observable behavior or exact `DomainError` variant. Run `cargo test -p <crate>` and observe it fail for the expected reason.
2. **Green**: Write the minimal clean Rust code needed to pass that test.
3. **Refactor**: Extract helpers to satisfy SLAP/Newspaper rules while staying Green.
4. **Repeat** for the next edge case or negative guard.

---

## 3. Non-Negotiable Testing Rules

- **No Tautological Assertions**: Never compute expected test values by repeating the production formula; use independent constants or simple spec oracles.
- **No Internal Collaboration Mocks**: Only use fakes/spies at architectural port seams (`SpyPublisher`, in-memory repository traits), never mock internal domain structs.
- **Match Error Variants Explicitly**: Assert `matches!(err, DomainError::CurrencyMismatch { .. })` or HTTP status + `code`, not fragile prose error messages.
- **`.unwrap()` / `.expect()` Scope**: `Cargo.toml` enforces `unwrap_used = "deny"` and `expect_used = "deny"` in production code. Per `clippy.toml` (`allow-unwrap-in-tests = true`, `allow-expect-in-tests = true`), both `.unwrap()` and `.expect("reason")` are permitted inside `#[test]` / `#[cfg(test)]`; helper functions outside `#[test]` in `tests/*.rs` integration tests must return `Result` and propagate errors into `#[test]` functions.
- **Verify Traceability & Lints**:
  ```bash
  cargo test -p domain --test spec_traceability
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  ```
