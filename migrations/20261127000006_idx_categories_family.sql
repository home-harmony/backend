-- no-transaction
CREATE INDEX ASYNC IF NOT EXISTS idx_categories_family ON ledger_categories (family_id);
