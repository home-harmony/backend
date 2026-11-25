-- no-transaction
CREATE TABLE IF NOT EXISTS account_balance_snapshots (
  id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  account_id      UUID          NOT NULL,
  family_id       UUID          NOT NULL REFERENCES family_families(id),
  year_month      CHAR(7)       NOT NULL,  -- 'YYYY-MM', e.g. '2026-03'
  closing_balance NUMERIC(19,4) NOT NULL,
  currency        CHAR(3)       NOT NULL,
  computed_at     TIMESTAMPTZ   NOT NULL DEFAULT now(),
  UNIQUE (account_id, year_month),
  FOREIGN KEY (family_id, account_id) REFERENCES payment_accounts(family_id, id)
);
