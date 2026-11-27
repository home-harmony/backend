-- no-transaction
CREATE INDEX ASYNC IF NOT EXISTS idx_tx_amendment_of ON ledger_transactions (family_id, amendment_of_id);
