-- no-transaction
CREATE INDEX ASYNC IF NOT EXISTS idx_loan_kinds_family ON debt_loan_kinds (family_id);
