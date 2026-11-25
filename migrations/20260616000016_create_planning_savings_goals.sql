-- no-transaction
CREATE TABLE IF NOT EXISTS planning_savings_goals (
  id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  family_id           UUID          NOT NULL REFERENCES family_families(id),
  name                VARCHAR(200)  NOT NULL,
  target_amount_value NUMERIC(19,4) NOT NULL,
  target_currency     CHAR(3)       NOT NULL,
  target_date         DATE,
  linked_account_id   UUID,
  color               VARCHAR(7),
  icon                VARCHAR(50),
  status              VARCHAR(20)   NOT NULL DEFAULT 'in_progress' CHECK (status IN ('in_progress','achieved','cancelled')),
  created_at          TIMESTAMPTZ   NOT NULL DEFAULT now(),
  deleted_at          TIMESTAMPTZ,
  UNIQUE (family_id, id),
  FOREIGN KEY (family_id, linked_account_id) REFERENCES payment_accounts(family_id, id)
);
