-- no-transaction
CREATE INDEX ASYNC IF NOT EXISTS idx_goal_contributions ON goal_contributions (goal_id, contributed_at);
