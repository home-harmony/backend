-- no-transaction
CREATE INDEX ASYNC IF NOT EXISTS idx_members_family ON family_members (family_id);
