-- no-transaction
CREATE TABLE IF NOT EXISTS debt_loan_payments (
  id                    UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  loan_id               UUID          NOT NULL REFERENCES debt_loans(id),
  family_id             UUID          NOT NULL REFERENCES family_families(id),
  paid_at               DATE          NOT NULL,
  amount_value          NUMERIC(19,4) NOT NULL,
  amount_currency       CHAR(3)       NOT NULL,
  principal_portion     NUMERIC(19,4) NOT NULL,
  interest_portion      NUMERIC(19,4) NOT NULL,
  remaining_balance     NUMERIC(19,4) NOT NULL,
  linked_transaction_id UUID REFERENCES ledger_transactions(id),
  created_at            TIMESTAMPTZ   NOT NULL DEFAULT now(),
  idempotency_key       UUID          NOT NULL UNIQUE
);
