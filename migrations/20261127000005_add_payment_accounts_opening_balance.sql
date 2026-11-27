-- no-transaction
ALTER TABLE payment_accounts ADD COLUMN IF NOT EXISTS opening_balance NUMERIC(19,4) NOT NULL DEFAULT 0;
