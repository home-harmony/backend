-- no-transaction
CREATE TABLE IF NOT EXISTS budget_monthly_budgets (
  id                          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  family_id                   UUID          NOT NULL REFERENCES family_families(id),
  year_month                  CHAR(7)       NOT NULL,  -- 'YYYY-MM'
  total_income_expected_value NUMERIC(19,4) NOT NULL,
  total_income_currency       CHAR(3)       NOT NULL,
  status                      VARCHAR(20)   NOT NULL DEFAULT 'draft' CHECK (status IN ('draft','active')),
  created_at                  TIMESTAMPTZ   NOT NULL DEFAULT now(),
  approved_at                 TIMESTAMPTZ,
  approved_by                 UUID REFERENCES family_members(id),
  UNIQUE (family_id, year_month)
);
