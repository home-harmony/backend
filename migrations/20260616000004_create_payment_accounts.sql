-- no-transaction
CREATE TABLE IF NOT EXISTS payment_accounts (
  id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  family_id           UUID          NOT NULL REFERENCES family_families(id),
  owner_member_id     UUID          NOT NULL,
  name                VARCHAR(200)  NOT NULL,
  kind                VARCHAR(20)   NOT NULL CHECK (kind IN ('credit_card','debit_card','cash','bank_account')),
  currency            CHAR(3)       NOT NULL,
  credit_limit        NUMERIC(19,4),             -- CreditCard only
  last_four           VARCHAR(4),
  bank_name           VARCHAR(100),
  color               VARCHAR(7),
  included_in_budget  BOOLEAN       NOT NULL DEFAULT true,
  created_at          TIMESTAMPTZ   NOT NULL DEFAULT now(),
  deleted_at          TIMESTAMPTZ,
  UNIQUE (family_id, id),
  FOREIGN KEY (family_id, owner_member_id) REFERENCES family_members(family_id, id)
);
