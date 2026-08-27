-- One row, one id, for "matches every token" -- on every box.
--
-- `0006`/`0007` each pick a winner from what a given database happens to hold, so
-- the lab and the live box resolved the same logical fingerprint onto different
-- ids (the lab merged onto `793c5b87`, the live box would keep its own
-- `isl-ALL broad` row). That is not a cosmetic difference:
-- `db-incremental-sync.ps1` mirrors `fingerprints` and `strategy_rules`
-- server-wins by PRIMARY KEY, so two ids for one match means the next sync
-- re-creates the duplicate the merge just removed and moves rules back onto it.
--
-- So the id is pinned rather than derived. Every axis-free, `{}`-config wildcard
-- collapses onto it, whichever box runs this.
--
-- `created_at` is the lab row's own, so both databases hold a byte-identical row
-- and the sync's `created_at = EXCLUDED.created_at` has nothing to rewrite.
INSERT INTO fingerprints (
    id, name, cu_limit, cu_price, init_buy_lamports, max_cost_lamports,
    spendable_lamports_in, first_slot_buy_lamports, first_slot_sell_lamports,
    bucket_size_amount, ix_labels, wildcard, metric_config, created_at, updated_at
) VALUES (
    '793c5b87-b33a-4c28-9147-7bef8a45e9f7', 'ALL', NULL, NULL, NULL, NULL,
    NULL, NULL, NULL, NULL, NULL, TRUE, '{}'::jsonb,
    TIMESTAMPTZ '2026-07-29 10:00:19.925474+00', now()
)
ON CONFLICT (id) DO NOTHING;

-- Move every rule off the other plain wildcards. Restricted to `metric_config =
-- '{}'` throughout: that column is not match identity, but it IS live (it compiles
-- into `m_flow_split` at reload), so the ten `8dtx · <router>` carriers and the
-- `8dtx-derived` classifier keep their own rows exactly as `0006` and `0007` left
-- them. They are match-identical to this one and stay separate on purpose.
UPDATE strategy_rules SET
    fingerprint_id = '793c5b87-b33a-4c28-9147-7bef8a45e9f7',
    updated_at = now()
WHERE fingerprint_id IN (
    SELECT id FROM fingerprints
    WHERE wildcard
      AND metric_config::text = '{}'
      AND id <> '793c5b87-b33a-4c28-9147-7bef8a45e9f7'
);

DELETE FROM fingerprints
WHERE wildcard
  AND metric_config::text = '{}'
  AND id <> '793c5b87-b33a-4c28-9147-7bef8a45e9f7';
