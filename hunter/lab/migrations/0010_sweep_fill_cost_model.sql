-- Fill model + cost model for a grouped sweep run.
--
-- The sweep used to hardcode the pessimistic worst-in-window fill AND charge
-- `slippage_bps` on top of it — a fill model that already prices slippage. That is
-- not a harmless constant pessimism: worst-in-slot penalises short holds hardest, and
-- the fixed per-leg cost scales with how often a combo fires, so the pair distorted
-- the very comparison a sweep exists to make.
--
-- Both are now request inputs, which makes them part of a run's IDENTITY: two runs
-- priced under different models are not comparable and the UI must say so, so they
-- are persisted here rather than left implicit.
--
-- NULL on legacy rows and read back as the old behaviour
-- (`worst_case` + `pumpfun_default`), so every stored run keeps the meaning it was
-- computed under. Stored as text (the serde tag) rather than an enum: these are wire
-- values owned by the Rust types, and a CHECK would have to be migrated in lockstep
-- with every new model.

ALTER TABLE grouped_sweep_runs
    ADD COLUMN IF NOT EXISTS fill_model TEXT,
    ADD COLUMN IF NOT EXISTS cost_model TEXT;
