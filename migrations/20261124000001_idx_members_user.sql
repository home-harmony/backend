-- no-transaction
CREATE INDEX ASYNC IF NOT EXISTS idx_members_user ON family_members (user_id);
