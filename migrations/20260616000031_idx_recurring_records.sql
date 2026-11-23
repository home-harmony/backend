-- no-transaction
CREATE INDEX ASYNC IF NOT EXISTS idx_recurring_records ON recurring_payment_records (recurring_payment_id, paid_at);
