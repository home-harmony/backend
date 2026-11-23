-- no-transaction
CREATE TABLE IF NOT EXISTS ledger_transactions (
  id                          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  family_id                   UUID          NOT NULL REFERENCES family_families(id),
  recorded_by                 UUID          NOT NULL REFERENCES family_members(id),
  kind                        VARCHAR(20)   NOT NULL CHECK (kind IN ('income','expense','transfer','cash_withdrawal','loan_payment','reversal')),
  amount_value                NUMERIC(19,4) NOT NULL,
  amount_currency             CHAR(3)       NOT NULL,
  source_account_id           UUID REFERENCES payment_accounts(id),       -- NULL for external income
  destination_account_id      UUID REFERENCES payment_accounts(id),       -- NULL for external expense
  destination_amount_value    NUMERIC(19,4),                              -- cross-currency transfer
  destination_amount_currency CHAR(3),                                    -- cross-currency transfer
  category_id                 UUID REFERENCES ledger_categories(id),
  amendment_of_id             UUID REFERENCES ledger_transactions(id),    -- NULL for original entries; points to corrected tx
  tags                        JSONB         NOT NULL DEFAULT '[]'::jsonb, -- JSON array of tags
  description                 TEXT,
  occurred_at                 TIMESTAMPTZ   NOT NULL,
  created_at                  TIMESTAMPTZ   NOT NULL DEFAULT now(),
  idempotency_key             UUID          NOT NULL UNIQUE,
  deleted_at                  TIMESTAMPTZ
);
