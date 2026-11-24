# Aurora DSQL Migration Files
#
# RULES (documentation/architecture/aurora-dsql.md + documentation/architecture/database-schema.md):
#   1. Start every file with `-- no-transaction` on line 1 (prevents sqlx from
#      combining DDL and `_sqlx_migrations` DML inside the same transaction).
#   2. Exactly ONE DDL statement per file. Never mix DDL and DML.
#   3. Use `CREATE TABLE IF NOT EXISTS` / `CREATE INDEX ASYNC IF NOT EXISTS` for idempotency.
#   4. Never use: SERIAL, BIGSERIAL, TRUNCATE, triggers.
#   5. All PKs must use: UUID PRIMARY KEY DEFAULT gen_random_uuid() (or UUID v7 from application code).
#   6. Explicit FOREIGN KEY (...) REFERENCES ... and native JSONB columns are supported.
#   7. All indexes must use: CREATE INDEX ASYNC (never blocking CREATE INDEX).
#   8. Never modify already-applied migration files. Add new ones instead.
#
# File naming convention:
#   {YYYYMMDDHHMMSS}_{description}.sql
#
# All schema table and async index migration files are placed here (chronologically ordered).
# See documentation/architecture/database-schema.md for the full table and index catalog.
