-- no-transaction
CREATE INDEX ASYNC IF NOT EXISTS idx_tx_account_dst ON ledger_transactions (destination_account_id, occurred_at);
