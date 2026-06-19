-- entry_price and entry_amount start as NULL (no fill yet) and are populated
-- by update_entry once the actual buy fill lands. Drop the NOT NULL constraint
-- so the Rust Option<f64> type matches the DB column nullability.
ALTER TABLE tpsl1_real_positions
    ALTER COLUMN entry_price DROP NOT NULL,
    ALTER COLUMN entry_amount DROP NOT NULL;

ALTER TABLE tpsl1_paper_positions
    ALTER COLUMN entry_price DROP NOT NULL,
    ALTER COLUMN entry_amount DROP NOT NULL;

ALTER TABLE tpsl2_real_positions
    ALTER COLUMN entry_price DROP NOT NULL,
    ALTER COLUMN entry_amount DROP NOT NULL;

ALTER TABLE tpsl2_paper_positions
    ALTER COLUMN entry_price DROP NOT NULL,
    ALTER COLUMN entry_amount DROP NOT NULL;
