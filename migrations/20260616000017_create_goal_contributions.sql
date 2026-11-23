-- no-transaction
CREATE TABLE IF NOT EXISTS goal_contributions (
  id                    UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  goal_id               UUID          NOT NULL REFERENCES planning_savings_goals(id),
  family_id             UUID          NOT NULL REFERENCES family_families(id),
  amount_value          NUMERIC(19,4) NOT NULL,
  amount_currency       CHAR(3)       NOT NULL,
  contributed_at        DATE          NOT NULL,
  linked_transaction_id UUID REFERENCES ledger_transactions(id),
  created_at            TIMESTAMPTZ   NOT NULL DEFAULT now(),
  idempotency_key       UUID          NOT NULL UNIQUE
);
