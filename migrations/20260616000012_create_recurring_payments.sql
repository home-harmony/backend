-- no-transaction
CREATE TABLE IF NOT EXISTS recurring_payments (
  id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  family_id            UUID          NOT NULL REFERENCES family_families(id),
  name                 VARCHAR(200)  NOT NULL,
  kind                 VARCHAR(20)   NOT NULL CHECK (kind IN ('utility','subscription','insurance','rent','other')),
  amount_value         NUMERIC(19,4) NOT NULL,
  amount_currency      CHAR(3)       NOT NULL,
  is_variable_amount   BOOLEAN       NOT NULL DEFAULT false,
  frequency            VARCHAR(20)   NOT NULL CHECK (frequency IN ('monthly','quarterly','annual','irregular')),
  frequency_config     JSONB         NOT NULL DEFAULT '{}'::jsonb,  -- JSON config
  payment_account_id   UUID,
  category_id          UUID          NOT NULL REFERENCES ledger_categories(id),
  linked_loan_id       UUID,
  next_due_date        DATE,
  is_active            BOOLEAN       NOT NULL DEFAULT true,
  notes                TEXT,
  created_at           TIMESTAMPTZ   NOT NULL DEFAULT now(),
  deleted_at           TIMESTAMPTZ,
  UNIQUE (family_id, id),
  FOREIGN KEY (family_id, payment_account_id) REFERENCES payment_accounts(family_id, id),
  FOREIGN KEY (family_id, linked_loan_id) REFERENCES debt_loans(family_id, id)
);
