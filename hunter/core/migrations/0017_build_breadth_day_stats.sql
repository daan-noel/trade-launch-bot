-- 0017: the daily build-breadth table (`m_holder_book.public_app_share`).
--
-- One row per (UTC day, build recipe): how many distinct wallets bought with the
-- recipe, on any token, on the PREVIOUS day. A recipe is a transaction's ordered
-- `ix_labels` without account setup, teardown and memos - the engine's
-- `flow_ix::build_hash` grain, stored as the recipe's labels and hashed at load.
-- Computed once a day off one day of `trades` (curve buys), written once and never
-- rewritten, so a restart never changes the class a holder was stamped with.
CREATE TABLE IF NOT EXISTS build_breadth_day_stats (
    day     DATE    NOT NULL,
    recipe  JSONB   NOT NULL,
    buyers  INTEGER NOT NULL,
    PRIMARY KEY (day, recipe)
);
