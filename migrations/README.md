# Aurora DSQL Migration Files
#
# RULES (GEMINI.md + Sprint Plan Task 3.1):
#   1. Exactly ONE DDL statement per file. Never mix DDL and DML.
#   2. Never use: FOREIGN KEY, SERIAL, BIGSERIAL, JSONB columns, TRUNCATE, triggers.
#   3. All PKs must use: UUID PRIMARY KEY DEFAULT gen_random_uuid()
#   4. All indexes must use: CREATE INDEX ASYNC (never blocking CREATE INDEX)
#   5. Store JSON data as TEXT columns; cast to ::jsonb at query time.
#   6. Never modify already-applied migration files. Add new ones instead.
#
# File naming convention (Flyway-style):
#   V{timestamp}_{description}.sql
#
# The 25 migration files for Sprint 1 (Task 3.1) will be added here.
# See the sprint plan for the full list of files.
