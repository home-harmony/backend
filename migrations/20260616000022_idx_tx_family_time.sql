-- no-transaction
CREATE INDEX ASYNC IF NOT EXISTS idx_tx_family_time ON ledger_transactions (family_id, occurred_at, id);
