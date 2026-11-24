# Aurora DSQL & Persistence Rules

> Applies to `backend/migrations/` and all persistence code in `backend/infrastructure/`.

---

## 1. Primary Keys & Data Integrity

- **Primary Keys**: UUID v4 (`gen_random_uuid()`) for low-frequency aggregates/tokens; time-ordered **UUID v7 (`Uuid::now_v7()`)** for transactions and append logs. Never use `SERIAL`, `BIGSERIAL`, or monotonic sequences.
- **Multi-Tenant Isolation**: Always qualify queries with `WHERE family_id = $1` derived from verified `AuthClaims` (`ADR-0011`), never from raw request payloads.
- **Schema Features**: Explicit `FOREIGN KEY (...) REFERENCES ...` and native `JSONB` columns are supported.
- **Soft-Deletes Only**: Use `deleted_at TIMESTAMPTZ NULL` (`ON DELETE RESTRICT`); never hard-delete business records or use `TRUNCATE` or triggers.

---

## 2. Migrations & DDL Invariants

- **Filename Format**: `{YYYYMMDDHHMMSS}_{description}.sql`.
- **Non-Transactional Idempotent DDL**: Start every migration file on line 1 with `-- no-transaction` and use `CREATE TABLE IF NOT EXISTS` / `CREATE INDEX ASYNC IF NOT EXISTS`. Disable SQLx advisory locks (`Migrator::set_locking(false)`) and await async builds via `CALL sys.wait_for_job` (`infrastructure::db::migrations`).
- **Exactly 1 DDL Statement per Migration File**: Never mix DDL and DML, and never combine multiple DDL statements in one file.
- **Never Edit Applied Migrations**: Always create a new migration file for schema changes.
- **Asynchronous Indexes**: All secondary indexes must use **`CREATE INDEX ASYNC`** in dedicated migration files (never blocking `CREATE INDEX`, and never include `ASC`/`DESC` sort modifiers).

---

## 3. Transactions, Queries & Pagination

- **OCC Retries**: Wrap mutating transactions in `aurora_dsql_sqlx_connector::retry_on_occ` (auto-retries `SQLSTATE 40001` / `OC000` / `OC001`); keep batch writes $\le 500$ rows.
- **Read-Only Endpoints**: Start read-only transactions directly with **`pool.begin_with("BEGIN READ ONLY")`** (Aurora DSQL does not support `SET TRANSACTION READ ONLY` after `BEGIN`).
- **Least-Privilege Runtime Role (`DSQL_USER`)**: Runtime service Lambdas (`FamilyLedgerServiceRole` with `dsql:DbConnect` only) must set `DSQL_USER` to an IAM-mapped non-admin DSQL role (reserve `"admin"` for `migrate_runner`).
- **Keyset Pagination Only**: `OFFSET` is strictly banned; use `(occurred_at, id) < ($2, $3)` with limit $\le 50$.
- **Parameterized Queries**: All SQL queries must be parameterized (`sqlx`); dynamic SQL string concatenation is strictly forbidden.
- **Offline CI Cache**: Run `cargo sqlx prepare --workspace -- --all-targets` after modifying compile-time checked queries.
