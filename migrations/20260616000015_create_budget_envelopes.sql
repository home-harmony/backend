-- no-transaction
CREATE TABLE IF NOT EXISTS budget_envelopes (
  id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  family_id           UUID          NOT NULL REFERENCES family_families(id),
  budget_id           UUID          NOT NULL,
  category_id         UUID          NOT NULL REFERENCES ledger_categories(id),
  limit_amount_value  NUMERIC(19,4) NOT NULL,
  limit_currency      CHAR(3)       NOT NULL,
  order_index         SMALLINT      NOT NULL DEFAULT 0,
  alert_threshold     SMALLINT      NOT NULL DEFAULT 80, -- percentage (80 = 80%)
  UNIQUE (budget_id, category_id),
  FOREIGN KEY (family_id, budget_id) REFERENCES budget_monthly_budgets(family_id, id)
);
