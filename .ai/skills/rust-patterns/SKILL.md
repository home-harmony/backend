---
name: rust-patterns
description: >-
  Use when designing, implementing, or refactoring Rust code in backend/.
  Covers Deep Modules (Deletion Test), SLAP/SRP structure, Newspaper ordering,
  RPITIT static dispatch ports, nutype value objects, Decimal math, ACL DTOs,
  and thiserror-to-API mapping.
---

# Rust Clean Code & Architectural Patterns

Load before designing, writing, or refactoring any crate in `backend/` (`domain`, `infrastructure`, `api`, `lambdas/*`).

---

## 1. Deep Modules & Hexagonal Seams

- **Deletion Test**: Before adding a struct, service wrapper, or trait, ask: *"If I delete this module, does its complexity disappear (shallow pass-through) or scatter across callers (deep module)?"* Eliminate shallow wrappers.
- **Hexagonal Boundaries**:
  - `domain/` owns entities, value objects, domain events, `DomainError`, and repository/provider port traits (zero `sqlx`, `axum`, filesystem, or AWS SDK imports).
  - `infrastructure/` implements repository traits, Aurora DSQL pools, EventBridge publishers, and FX providers.
  - `api/` and `lambdas/*` handle Cognito JWT extraction, `X-Family-Id` membership verification, HTTP DTOs, and error translation.
- **Async Ports via RPITIT & Static Dispatch**:
  - Define async domain traits using native RPITIT with explicit `+ Send` (keeping domain traits `Send + Sync`, free of `'static`):
    ```rust
    pub trait AccountRepository: Send + Sync {
        fn find_by_id(&self, family_id: FamilyId, id: AccountId)
            -> impl Future<Output = Result<Option<PaymentAccount>, DomainError>> + Send;
    }
    ```
  - Inject ports via generic type parameters (`struct Service<R> { repo: R }`) for zero-cost static dispatch with both production adapters and test fakes. Reserve `Arc<dyn Trait>` for synchronous traits or open registries.

---

## 2. SLAP, SRP & Newspaper File Layout

- **Single Responsibility & Single Level of Abstraction (SLAP)**:
  - **Orchestrators**: Coordinate high-level domain or handler steps via well-named helpers; contain zero inline SQL mapping, loop math, or string parsing.
  - **Workers**: Perform one focused low-level task (e.g., cursor decoding, row-to-entity mapping, amortization step).
- **Function Metrics**: Target **10–30 lines** per function (hard ceiling **40 lines**); max nesting **2 levels** using early guard clauses (`?`, `let ... else { return ... }`).
- **Newspaper Ordering (Top-to-Bottom in Every `.rs` File)**:
  1. **Imports**: `std` $\rightarrow$ external crates $\rightarrow$ workspace/local crates (`domain`, `crate::...`)
  2. **Types**: Constants, type aliases, structs, enums
  3. **Implementations**: Inherent `impl`s first, trait `impl`s second
  4. **Functions**: Public entry points/orchestrators first, private leaf helpers below
  5. **Tests**: `#[cfg(test)] mod tests { ... }` at the bottom

---

## 3. Domain Modeling & Value Object Idioms

- **Mandatory `nutype` & Typed IDs**: Validate scalar invariants at construction (`CurrencyCode::new`, `FamilyName::try_new`, `DisplayName::try_new`, `Email::try_new`). Never pass raw `String`, `Uuid`, or `i64` across domain boundaries.
- **Exact Money**: Always use `rust_decimal::Decimal` wrapped in `Money` (never `f32`/`f64`).
- **Enum State Machines & Typestates**: Model aggregate lifecycles (`MonthlyBudget` `Draft`/`Active`, `Transaction` kinds) as enums carrying state-specific data or transitions returning `Result<Self, DomainError>`.
- **Anti-Corruption Layer (ACL)**: Keep `serde` wire DTOs and `sqlx::FromRow` structs out of `domain/` when external representations differ; translate via explicit `TryFrom` or dedicated mapper functions.
- **Error Propagation (`thiserror`)**: Define cohesive error enums (`DomainError`) and map them centrally at the API boundary (`api/src/errors.rs`). Never use `.unwrap()` or `.expect()` in production code.
- **Deterministic Collections**: Use `indexmap::{IndexMap, IndexSet}` whenever field or schedule ordering must remain deterministic.

---

## 4. Implementation Sequence

1. Confirm the test seams (`rust-testing`) and inspect relevant `backend/specs/` rules.
2. Separate orchestrators from leaf helpers and design `nutype`/enum invariants first.
3. Implement vertically and verify continuously:
   ```bash
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```
4. Execute `code-review` before finishing.
