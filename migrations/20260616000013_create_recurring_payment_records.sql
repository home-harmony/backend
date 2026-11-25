-- no-transaction
CREATE TABLE IF NOT EXISTS recurring_payment_records (
  id                      UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  recurring_payment_id    UUID          NOT NULL,
  family_id               UUID          NOT NULL REFERENCES family_families(id),
  actual_amount_value     NUMERIC(19,4) NOT NULL,
  actual_amount_currency  CHAR(3)       NOT NULL,
  paid_at                 DATE          NOT NULL,
  linked_transaction_id   UUID,
  created_at              TIMESTAMPTZ   NOT NULL DEFAULT now(),
  idempotency_key         UUID          NOT NULL,
  UNIQUE (family_id, idempotency_key),
  FOREIGN KEY (family_id, recurring_payment_id) REFERENCES recurring_payments(family_id, id),
  FOREIGN KEY (family_id, linked_transaction_id) REFERENCES ledger_transactions(family_id, id)
);
