-- no-transaction
CREATE INDEX ASYNC IF NOT EXISTS idx_loans_family ON debt_loans (family_id, status);
