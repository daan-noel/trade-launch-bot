-- 0018: the build-breadth table carries each recipe's APP (`m_holder_book.public_app_share`).
--
-- A public app is wide AND its wallets come back: a bot swarm spreads thousands of
-- wallets over many programs at about one buy per wallet per program a day, where
-- the named apps run 2.9 and up (hot-tape case file L12). Per row, the recipe's app -
-- the first program past compute budget, system, token, associated-token and memo;
-- the recipe itself for a direct pump.fun call - counted across every recipe it sent
-- on the PREVIOUS day: its distinct buying wallets and its buy transactions.
--
-- A 0017 row holds only its recipe's own breadth, so they are dropped here and each
-- day is recomputed from `trades` on its next load.
TRUNCATE build_breadth_day_stats;

ALTER TABLE build_breadth_day_stats
    DROP COLUMN buyers,
    ADD COLUMN app_buyers INTEGER NOT NULL,
    ADD COLUMN app_buys   INTEGER NOT NULL;
