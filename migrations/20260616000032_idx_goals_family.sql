-- no-transaction
CREATE INDEX ASYNC IF NOT EXISTS idx_goals_family ON planning_savings_goals (family_id, status);
