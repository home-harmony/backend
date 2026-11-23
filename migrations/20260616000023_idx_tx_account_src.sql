-- no-transaction
CREATE INDEX ASYNC IF NOT EXISTS idx_tx_account_src ON ledger_transactions (source_account_id, occurred_at);
