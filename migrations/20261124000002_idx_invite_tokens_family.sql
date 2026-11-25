-- no-transaction
CREATE INDEX ASYNC IF NOT EXISTS idx_invite_tokens_family ON family_invite_tokens (family_id);
