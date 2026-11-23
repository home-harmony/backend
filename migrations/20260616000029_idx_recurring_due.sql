-- no-transaction
CREATE INDEX ASYNC IF NOT EXISTS idx_recurring_due ON recurring_payments (next_due_date) WHERE is_active = true;
