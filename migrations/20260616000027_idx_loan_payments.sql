-- no-transaction
CREATE INDEX ASYNC IF NOT EXISTS idx_loan_payments ON debt_loan_payments (loan_id, paid_at);
