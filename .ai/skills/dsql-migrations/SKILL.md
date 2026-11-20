---
name: dsql-migrations
description: >-
  Use when creating or modifying SQL migrations in backend/migrations/ or writing
  SQLx queries and repository implementations in backend/infrastructure/.
  Enforces Aurora DSQL 1-DDL-per-file rules, CREATE INDEX ASYNC, OCC retry,
  BEGIN READ ONLY, keyset pagination, and sqlx offline preparation.
---

# Aurora DSQL Migrations & Persistence Runbook

Procedural runbook for authoring migrations in `backend/migrations/` and SQLx repository adapters in `backend/infrastructure/`. Enforces all invariants from [`aurora-dsql.md`](../../rules/aurora-dsql.md), [`documentation/architecture/aurora-dsql.md`](../../../../documentation/architecture/aurora-dsql.md), and [`migrations/README.md`](../../../migrations/README.md).

---

## 1. Migration Authoring Workflow (`backend/migrations/`)

1. **Determine Timestamp Prefix**: List `backend/migrations/` and pick the next chronological `{YYYYMMDDHHMMSS}_{description}.sql` filename. Never edit or delete applied migrations.
2. **Split DDL Cleanly**: Create **one file per DDL statement** (`CREATE TABLE`, `ALTER TABLE`, or `CREATE INDEX ASYNC`)—never mix DDL/DML or batch multiple statements in one file.
3. **Verify Aurora DSQL Compatibility**: Confirm UUID v4/v7 PKs (zero sequences), `family_id UUID NOT NULL`, `deleted_at TIMESTAMPTZ NULL`, `CREATE INDEX ASYNC`, and zero `TRUNCATE`/triggers/stored procedures (per [`aurora-dsql.md`](../../rules/aurora-dsql.md)).

---

## 2. Repository Adapter Workflow (`backend/infrastructure/`)

1. **Scope Every Query**: Filter `WHERE family_id = $1` (from verified `AuthClaims`) and `AND deleted_at IS NULL` unless explicitly querying soft-deleted rows.
2. **Select Transaction Mode**:
   - **Read endpoints**: Wrap in `BEGIN READ ONLY`.
   - **Mutations**: Wrap in `aurora_dsql_sqlx_connector::retry_on_occ` (`SQLSTATE 40001`), keeping batches $\le 500$ rows.
   - **Ledger writes**: Append `TransactionAmendment` (`Reversal` + corrected entry) rather than `UPDATE`/`DELETE`.
3. **Paginate via Keyset Cursor**: Use `infrastructure::db::pagination::{Page, PageCursor}` with `(occurred_at, id) < ($2, $3) ORDER BY occurred_at DESC, id DESC LIMIT $4` ($\le 50$ items; zero `OFFSET`).
4. **Keep SQL Parameterized**: Use compile-time checked `sqlx` bindings only (no string interpolation).

---

## 3. Verification Steps

1. Run database integration tests (with Docker running for Testcontainers PG):
   ```bash
   cargo test -p infrastructure
   ```
2. Refresh the SQLx offline verification cache whenever compile-time queries change:
   ```bash
   cargo sqlx prepare -- --all-targets
   ```
3. Run workspace verification (`cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`) and `code-review`.
