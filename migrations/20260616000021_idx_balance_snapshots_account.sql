-- no-transaction
CREATE INDEX ASYNC IF NOT EXISTS idx_balance_snapshots_account ON account_balance_snapshots (account_id, year_month);
