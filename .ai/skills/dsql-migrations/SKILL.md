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
2. **Split DDL Cleanly**: Start line 1 with `-- no-transaction` and write **one DDL statement per file** (`CREATE TABLE IF NOT EXISTS`, `ALTER TABLE`, or `CREATE INDEX ASYNC IF NOT EXISTS`)—never mix DDL/DML or batch multiple statements in one file.
3. **Verify Aurora DSQL Compatibility**: Confirm UUID v4/v7 PKs (zero sequences), `family_id UUID NOT NULL`, `deleted_at TIMESTAMPTZ NULL`, `CREATE INDEX ASYNC` (no `ASC`/`DESC` sort modifiers in index definitions), and zero `TRUNCATE`/triggers/stored procedures (per [`aurora-dsql.md`](../../rules/aurora-dsql.md)).

---

## 2. Repository Adapter Workflow (`backend/infrastructure/`)

1. **Scope Every Query**: Filter `WHERE family_id = $1` (from verified `AuthClaims`) and `AND deleted_at IS NULL` unless explicitly querying soft-deleted rows.
2. **Select Transaction Mode**:
   - **Read endpoints**: Start transactions directly with `pool.begin_with("BEGIN READ ONLY")` (Aurora DSQL rejects `SET TRANSACTION READ ONLY` after `BEGIN`).
   - **Mutations**: Wrap in `aurora_dsql_sqlx_connector::retry_on_occ` (`SQLSTATE 40001`), keeping batches $\le 500$ rows.
   - **Ledger writes**: Append `TransactionAmendment` (`Reversal` + corrected entry) rather than `UPDATE`/`DELETE`.
3. **Paginate via Keyset Cursor**: Use `infrastructure::db::pagination::{Page, PageCursor}` with `(occurred_at, id) < ($2, $3) ORDER BY occurred_at DESC, id DESC LIMIT $4` ($\le 50$ items; zero `OFFSET`).
4. **Keep SQL Parameterized**: Use compile-time checked `sqlx` bindings only (no string interpolation).

---

## 3. Testing, Building & Deploying Migrations

1. **Test Offline Invariants & Local PostgreSQL 16 (Testcontainers)**:
   ```powershell
   # Fast offline invariant checks (no Docker required)
   cargo test -p infrastructure --test migration_invariants

   # Full PostgreSQL 16 integration test (requires Docker active)
   cargo test -p infrastructure -- --ignored
   ```
2. **Build, Deploy & Invoke Migration Runner on AWS Aurora DSQL**:
   *(Requires `cargo-lambda >= 1.9.2` and `zig >= 0.17.1` on Windows: `scoop update cargo-lambda zig`.)*
   ```powershell
   # 1. Cross-compile ARM64 Lambda binary (target/lambda/migrate_runner/bootstrap)
   cargo lambda build --release --arm64 --package migrate_runner

   # 2. Validate & deploy familyledger-data-dev via SAM
   cd infrastructure\sam
   sam validate -t data-stream-template.yaml --lint
   sam deploy --template-file data-stream-template.yaml --config-file samconfig-data.toml --capabilities CAPABILITY_NAMED_IAM --no-confirm-changeset
   cd ..\..

   # 3. Invoke familyledger-migrate-dev on AWS & inspect result
   aws lambda invoke --function-name familyledger-migrate-dev --region us-east-1 --log-type Tail --cli-binary-format raw-in-base64-out --payload '{}' target\migrate-response.json
   Get-Content target\migrate-response.json
   Remove-Item target\migrate-response.json -Force
   ```
3. **Refresh SQLx Offline Query Cache** (when compile-time `sqlx::query!` macros change; install CLI via `cargo install sqlx-cli@0.9.0 --no-default-features --features postgres,rustls`):
   ```powershell
   cargo sqlx prepare -- --all-targets
   ```
4. **Run Workspace Verification Gates**: `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `code-review`.
