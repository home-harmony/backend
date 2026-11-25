-- no-transaction
CREATE TABLE IF NOT EXISTS debt_loan_payments (
  id                    UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  loan_id               UUID          NOT NULL,
  family_id             UUID          NOT NULL REFERENCES family_families(id),
  paid_at               DATE          NOT NULL,
  amount_value          NUMERIC(19,4) NOT NULL,
  amount_currency       CHAR(3)       NOT NULL,
  principal_portion     NUMERIC(19,4) NOT NULL,
  interest_portion      NUMERIC(19,4) NOT NULL,
  remaining_balance     NUMERIC(19,4) NOT NULL,
  linked_transaction_id UUID,
  created_at            TIMESTAMPTZ   NOT NULL DEFAULT now(),
  idempotency_key       UUID          NOT NULL,
  UNIQUE (family_id, idempotency_key),
  FOREIGN KEY (family_id, loan_id) REFERENCES debt_loans(family_id, id),
  FOREIGN KEY (family_id, linked_transaction_id) REFERENCES ledger_transactions(family_id, id)
);
