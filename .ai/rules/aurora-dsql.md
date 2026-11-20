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
- **Exactly 1 DDL Statement per Migration File**: Never mix DDL and DML, and never combine multiple DDL statements in one file.
- **Never Edit Applied Migrations**: Always create a new migration file for schema changes.
- **Asynchronous Indexes**: All secondary indexes must use **`CREATE INDEX ASYNC`** in dedicated migration files (never blocking `CREATE INDEX`).

---

## 3. Transactions, Queries & Pagination

- **OCC Retries**: Wrap mutating transactions in `aurora_dsql_sqlx_connector::retry_on_occ` (auto-retries `SQLSTATE 40001`); keep batch writes $\le 500$ rows.
- **Read-Only Endpoints**: Use **`BEGIN READ ONLY`** for all read-only queries/endpoints.
- **Keyset Pagination Only**: `OFFSET` is strictly banned; use `(occurred_at, id) < ($2, $3)` with limit $\le 50$.
- **Parameterized Queries**: All SQL queries must be parameterized (`sqlx`); dynamic SQL string concatenation is strictly forbidden.
- **Offline CI Cache**: Run `cargo sqlx prepare -- --all-targets` after modifying compile-time checked queries.
