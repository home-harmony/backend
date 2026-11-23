-- no-transaction
CREATE INDEX ASYNC IF NOT EXISTS idx_recurring_linked_loan ON recurring_payments (linked_loan_id) WHERE linked_loan_id IS NOT NULL;
