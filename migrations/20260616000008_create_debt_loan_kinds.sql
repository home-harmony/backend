-- no-transaction
CREATE TABLE IF NOT EXISTS debt_loan_kinds (
  id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  family_id   UUID REFERENCES family_families(id), -- NULL = system-wide standard kind
  code        VARCHAR(50)  NOT NULL,
  name        VARCHAR(200) NOT NULL,
  is_system   BOOLEAN      NOT NULL DEFAULT false,
  sort_order  SMALLINT     NOT NULL DEFAULT 0,
  deleted_at  TIMESTAMPTZ,
  UNIQUE (family_id, code)
);
