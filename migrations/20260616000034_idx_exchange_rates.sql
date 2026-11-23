-- no-transaction
CREATE INDEX ASYNC IF NOT EXISTS idx_exchange_rates ON exchange_rates (base_currency, quote_currency, fetched_at);
