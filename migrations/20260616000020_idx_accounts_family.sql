-- no-transaction
CREATE INDEX ASYNC IF NOT EXISTS idx_accounts_family ON payment_accounts (family_id);
