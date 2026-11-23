-- no-transaction
CREATE INDEX ASYNC IF NOT EXISTS idx_recurring_family ON recurring_payments (family_id, is_active);
