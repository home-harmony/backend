# Aurora DSQL Migration Files
#
# RULES (documentation/architecture/aurora-dsql.md + Sprint Plan Task 3.1):
#   1. Exactly ONE DDL statement per file. Never mix DDL and DML.
#   2. Never use: SERIAL, BIGSERIAL, TRUNCATE, triggers.
#   3. All PKs must use: UUID PRIMARY KEY DEFAULT gen_random_uuid() (or UUID v7 from application code).
#   4. Explicit FOREIGN KEY (...) REFERENCES ... and native JSONB columns are supported.
#   5. All indexes must use: CREATE INDEX ASYNC (never blocking CREATE INDEX).
#   6. Never modify already-applied migration files. Add new ones instead.
#
# File naming convention:
#   {YYYYMMDDHHMMSS}_{description}.sql
#
# The 34 migration files for Sprint 1 (Task 3.1) are placed here.
# See documentation/roadmap/sprint-1/task_3_1_detailed_guide.md for the full list of files.
