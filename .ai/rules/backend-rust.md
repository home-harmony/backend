# Backend (Rust) Rules

> Applies to all Rust crates in `backend/` (`domain`, `infrastructure`, `api`, `lambdas/*`).

---

## 1. Hexagonal Crate Boundaries, Deep Modules & Ports

- **Pure Domain (`domain/`)**: Zero SQL (`sqlx`), HTTP (`axum`), filesystem, or AWS SDK dependencies.
- **Deep Modules & Deletion Test**: Prefer deep modules that hide genuine complexity behind narrow interfaces. Avoid shallow pass-through wrappers whose complexity disappears if deleted.
- **Port Placement & Static Dispatch (RPITIT)**:
  - Declare repository/service port traits in `domain/` using native async RPITIT (`fn find(...) -> impl Future<Output = ...> + Send`).
  - Inject dependencies via generic type parameters (static dispatch) rather than `Arc<dyn Trait>` where possible; keep domain ports `Send + Sync` (free of `'static`), and require `'static` only on Axum HTTP state/application service traits.
- **Anti-Corruption Layer (ACL)**: Wire DTOs and DB row structs live in `api/` and `infrastructure/`; translate across layer boundaries explicitly.
- **Concurrency**: `tokio::spawn` and background tasks belong exclusively in `infrastructure/` or `lambdas/*`, never in `domain/`.

---

## 2. Type Safety & Value Objects

- **Mandatory `nutype` / Typed IDs**: Every domain identifier (`FamilyId`, `MemberId`, `TransactionId`) and constrained value object (`FamilyName`, `DisplayName`, `Email`, `CurrencyCode`) must use `nutype` or strongly-typed UUID newtypes. Never pass raw `String`, `Uuid`, or `i64` in domain signatures.
- **Exact Money**: Monetary values must always use `rust_decimal::Decimal` (`Money` VO). **`f32` and `f64` are strictly banned.**
- **Deterministic Collections**: Use `indexmap::{IndexMap, IndexSet}` when field or insertion order must be preserved.

---

## 3. Strict Prohibitions & Anti-Patterns

- **Zero `.unwrap()` or `.expect()` in Production Code**: Propagate errors using `?` and `thiserror`. (`.expect("reason")` is allowed only in test setup per `clippy.toml`).
- **No `Deref` Polymorphism**: Never implement `Deref` to simulate inheritance.
- **No Unjustified `.clone()`**: Do not call `.clone()` merely to silence the borrow checker; prefer borrowed slices (`&str`, `&[T]`) or moving owned values.
- **No `#![deny(warnings)]` in Crates**: Enforce zero warnings via `cargo clippy --workspace --all-targets -- -D warnings`.

---

## 4. Clean Code Metrics (SRP / SLAP) & Newspaper File Layout

- **Function Metrics**:
  - Target: **10–30 lines** per function (hard ceiling: **40 lines**).
  - Max nesting depth: **2 levels**. Use guard clauses (`?`, `let ... else { return ... }`) to fail fast at the top.
- **Single Level of Abstraction (SLAP)**: High-level orchestrators coordinate workflow steps; private worker helpers handle low-level parsing, loops, or SQL mapping.
- **Newspaper File Ordering** (top-to-bottom in every `.rs` file):
  1. **Imports**: `std` $\rightarrow$ external crates $\rightarrow$ workspace crates (`domain`, `crate::...`)
  2. **Types**: Constants, statics, type aliases, structs, enums
  3. **Implementations**: Inherent `impl`s first, trait `impl`s second
  4. **Functions**: Public entry points/orchestrators first, followed top-down by private leaf helpers
  5. **Tests**: `#[cfg(test)] mod tests { ... }` at the bottom

---

## 5. Testing Boundaries

- **Spec-Backed Tests (`<crate>/tests/*.rs`)**: Observable behavior tests derived from `Approved` rules in `backend/specs/` live in integration test targets (public API + test fakes only).
- **Private Unit Tests (`#[cfg(test)] mod tests`)**: Reserved for private internal helpers or algorithms with no public surface.
- **Zero Infrastructure Bleed in `domain`**: `domain/tests/` must never import `sqlx`, `axum`, or live DB pools.
