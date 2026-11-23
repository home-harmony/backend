-- no-transaction
CREATE TABLE IF NOT EXISTS exchange_rates (
  id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  base_currency  CHAR(3)       NOT NULL,
  quote_currency CHAR(3)       NOT NULL,
  rate           NUMERIC(18,8) NOT NULL,
  fetched_at     TIMESTAMPTZ   NOT NULL,
  UNIQUE (base_currency, quote_currency, fetched_at)
);
