# Backend (Rust & Aurora DSQL) Rules

> Applies to all Rust crates in `backend/` (`domain`, `infrastructure`, `api`, `lambdas/*`) and SQL migrations in `backend/migrations/`.

---

## 1. Hexagonal Crate Boundaries & Ports

- **Pure Domain (`domain/`)**: Zero SQL (`sqlx`), HTTP (`axum`), filesystem, or AWS SDK dependencies.
- **Port Placement & Static Dispatch (RPITIT)**:
  - Declare repository/service port traits in `domain/` using native async RPITIT (`fn find(...) -> impl Future<Output = ...> + Send`).
  - Inject dependencies via generic type parameters (static dispatch) rather than `Arc<dyn Trait>` where possible; keep domain ports `Send + Sync` (free of `'static`), and require `'static` only on Axum HTTP state/application service traits.
- **Concurrency**: `tokio::spawn` and background tasks belong exclusively in `infrastructure/`, never in `domain/`.

---

## 2. Aurora DSQL & Persistence Invariants

- **Primary Keys**: UUID v4 (`gen_random_uuid()`) for low-frequency aggregates/tokens; time-ordered **UUID v7 (`Uuid::now_v7()`)** for transactions and append logs. Never use `SERIAL` / monotonic sequences.
- **Schema Features**: Explicit `FOREIGN KEY (...) REFERENCES ...` and native `JSONB` columns are supported; always qualify queries with `WHERE family_id = $1`.
- **Soft-Deletes**: Use `deleted_at TIMESTAMPTZ NULL` (`ON DELETE RESTRICT`); never hard-delete business records or use `TRUNCATE`.
- **Migrations & DDL**:
  - Filename format: `{YYYYMMDDHHMMSS}_{description}.sql`.
  - **Exactly 1 DDL statement per migration file**. Never mix DDL and DML. Never edit already-applied migrations.
  - All secondary indexes must use **`CREATE INDEX ASYNC`** in dedicated migration files.
- **Transactions & Pagination**:
  - Wrap mutating transactions in `aurora_dsql_sqlx_connector::retry_on_occ` (auto-retries SQLSTATE `40001`); keep batch writes $\le 500$ rows.
  - Use **`BEGIN READ ONLY`** for all read-only endpoints.
  - **Keyset Pagination Only**: `OFFSET` is strictly banned; use `(occurred_at, id) < ($2, $3)` with limit $\le 50$.
  - All SQL queries must be parameterized (`sqlx`); dynamic SQL string concatenation is forbidden.

---

## 3. Type Safety & Value Objects

- **Mandatory `nutype` / Typed IDs**: Every domain identifier (`FamilyId`, `MemberId`, `TransactionId`) and constrained value object (`FamilyName`, `DisplayName`, `Email`) must use `nutype` or strongly-typed UUID newtypes. Never pass raw `String`, `Uuid`, or `i64` in domain signatures.
- **Exact Money**: Monetary values must always use `rust_decimal::Decimal` (`Money` VO). **`f32` and `f64` are strictly banned.**
- **Deterministic Collections**: Use `indexmap::{IndexMap, IndexSet}` when field or insertion order must be preserved.

---

## 4. Strict Prohibitions & Anti-Patterns

- **Zero `.unwrap()` or `.expect()` in Production Code**: Propagate errors using `?` and `thiserror`. (`.expect("reason")` is allowed only in test setup).
- **No `Deref` Polymorphism**: Never implement `Deref` to simulate inheritance.
- **No Unjustified `.clone()`**: Do not call `.clone()` merely to silence the borrow checker; prefer borrowed slices (`&str`, `&[T]`) or moving owned values.
- **No `#![deny(warnings)]` in Crates**: Enforce zero warnings via `cargo clippy --workspace --all-targets -- -D warnings`.

---

## 5. Clean Code Metrics (SRP / SLAP) & Newspaper File Layout

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
