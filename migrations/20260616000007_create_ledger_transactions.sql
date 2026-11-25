-- no-transaction
CREATE TABLE IF NOT EXISTS ledger_transactions (
  id                          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  family_id                   UUID          NOT NULL REFERENCES family_families(id),
  recorded_by                 UUID          NOT NULL,
  kind                        VARCHAR(20)   NOT NULL CHECK (kind IN ('income','expense','transfer','cash_withdrawal','loan_payment','reversal')),
  amount_value                NUMERIC(19,4) NOT NULL,
  amount_currency             CHAR(3)       NOT NULL,
  source_account_id           UUID,                                       -- NULL for external income
  destination_account_id      UUID,                                       -- NULL for external expense
  destination_amount_value    NUMERIC(19,4),                              -- cross-currency transfer
  destination_amount_currency CHAR(3),                                    -- cross-currency transfer
  category_id                 UUID REFERENCES ledger_categories(id),
  amendment_of_id             UUID,                                       -- NULL for original entries; points to corrected tx
  tags                        JSONB         NOT NULL DEFAULT '[]'::jsonb, -- JSON array of tags
  description                 TEXT,
  occurred_at                 TIMESTAMPTZ   NOT NULL,
  created_at                  TIMESTAMPTZ   NOT NULL DEFAULT now(),
  idempotency_key             UUID          NOT NULL,
  deleted_at                  TIMESTAMPTZ,
  UNIQUE (family_id, id),
  UNIQUE (family_id, idempotency_key),
  FOREIGN KEY (family_id, recorded_by) REFERENCES family_members(family_id, id),
  FOREIGN KEY (family_id, source_account_id) REFERENCES payment_accounts(family_id, id),
  FOREIGN KEY (family_id, destination_account_id) REFERENCES payment_accounts(family_id, id),
  FOREIGN KEY (family_id, amendment_of_id) REFERENCES ledger_transactions(family_id, id)
);
