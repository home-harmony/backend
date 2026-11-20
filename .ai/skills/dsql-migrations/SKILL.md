---
name: dsql-migrations
description: >-
  Use when creating or modifying SQL migrations in backend/migrations/ or writing
  SQLx queries and repository implementations in backend/infrastructure/.
  Enforces Aurora DSQL 1-DDL-per-file rules, CREATE INDEX ASYNC, OCC retry,
  BEGIN READ ONLY, keyset pagination, and sqlx offline preparation.
---

# Aurora DSQL Migrations & Persistence Runbook

Follow this runbook whenever authoring migrations in `backend/migrations/` or SQLx repository adapters in `backend/infrastructure/`. Reference [`aurora-dsql.md`](../../../documentation/architecture/aurora-dsql.md) and [`migrations/README.md`](../../../migrations/README.md).

---

## 1. Authoring Migration Files (`backend/migrations/`)

1. **Inspect Existing Migrations**: List `backend/migrations/` to determine the next chronological timestamp prefix (`{YYYYMMDDHHMMSS}_{description}.sql`). Never modify or delete already-applied migrations.
2. **Strict 1-DDL-Per-File Rule**:
   - Put **exactly ONE DDL statement** (`CREATE TABLE ...`, `ALTER TABLE ...`, or `CREATE INDEX ASYNC ...`) in each `.sql` file.
   - **Never** mix DDL and DML (`INSERT`/`UPDATE`), and never wrap DDL in multi-statement transaction blocks.
3. **Aurora DSQL Compatibility Checklist**:
   - Primary keys: `UUID PRIMARY KEY DEFAULT gen_random_uuid()` (for low-frequency tables) or application-supplied UUID v7 (`transactions`, append logs). **Zero `SERIAL` / `BIGSERIAL` / sequences.**
   - Secondary indexes: Always **`CREATE INDEX ASYNC`** in its own dedicated migration file.
   - Multi-tenancy & Soft-delete: Include `family_id UUID NOT NULL` (`FOREIGN KEY` supported) and `deleted_at TIMESTAMPTZ NULL` (`ON DELETE RESTRICT`).
   - Prohibited SQL features: No `TRUNCATE`, no triggers, no stored procedures.

---

## 2. Writing SQLx Queries & Repository Adapters (`backend/infrastructure/`)

1. **Tenant Isolation**: Every query must scope to `WHERE family_id = $1` (verified from `AuthClaims`, never raw client input) and filter `AND deleted_at IS NULL` unless explicitly querying archived/deleted records.
2. **Read vs. Write Transaction Semantics**:
   - **Reads**: Wrap read-only queries/endpoints in **`BEGIN READ ONLY`**.
   - **Writes**: Wrap mutating transactions in **`aurora_dsql_sqlx_connector::retry_on_occ`** (auto-retries `SQLSTATE 40001`), keeping batches $\le 500$ rows.
   - **Ledger Immutability**: Never emit `UPDATE` or `DELETE` on `ledger_transactions`; use `TransactionAmendment` (`Reversal` + new entry).
3. **Keyset Pagination Only**:
   - Strictly ban `OFFSET`.
   - Use opaque Base64 cursor pagination (`(occurred_at, id) < ($2, $3) ORDER BY occurred_at DESC, id DESC LIMIT $4`) capped at $\le 50$ items.
4. **Parameterized SQL Only**: Use compile-time checked `sqlx` macros/bindings; dynamic SQL string interpolation is forbidden.

---

## 3. Verification Steps

1. Run database integration tests (with Docker running for Testcontainers PG):
   ```bash
   cargo test -p infrastructure
   ```
2. Update the SQLx offline verification cache whenever queries change:
   ```bash
   cargo sqlx prepare -- --all-targets
   ```
3. Run workspace lints (`cargo clippy --workspace --all-targets -- -D warnings`) and `code-review`.
