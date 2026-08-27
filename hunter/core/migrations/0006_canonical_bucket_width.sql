-- Canonicalise the fingerprint bucket width, then merge the rows it duplicated.
--
-- `bucket_size_amount` only ever reaches a match through one of the five
-- bucket-matched SOL axes. With none configured it is inert -- it changes no
-- match -- but both readers of the column treated it as if it did:
--
--   * `FingerprintRepo::IDENTITY_WHERE` keys on it, so `find_or_create` minted a
--     new row instead of reusing one the engine already matches identically;
--   * `Fingerprint::auto_name` printed it, so one match carried several names.
--
-- Together those turned one fingerprint into several rows under several labels.
-- `Fingerprint::effective_bucket_size_amount` is now the one reader, every write
-- edge stores it, and the CHECK at the bottom is the backstop.

-- 1. Drop the inert width. No match changes: with no SOL axis every `sol_axis`
--    call already short-circuits on the `None` fingerprint value.
UPDATE fingerprints SET bucket_size_amount = NULL, updated_at = now()
WHERE bucket_size_amount IS NOT NULL
  AND init_buy_lamports IS NULL
  AND max_cost_lamports IS NULL
  AND spendable_lamports_in IS NULL
  AND first_slot_buy_lamports IS NULL
  AND first_slot_sell_lamports IS NULL;

-- 2. Strip the width chip from the names that printed it. Bounded to the rows
--    step 1 just canonicalised and to the exact trailing chip `auto_name` emits,
--    so a nickname that merely contains the text is untouched.
UPDATE fingerprints SET name = regexp_replace(name, ' · bkt=[^ ·]+$', ''), updated_at = now()
WHERE bucket_size_amount IS NULL
  AND init_buy_lamports IS NULL
  AND max_cost_lamports IS NULL
  AND spendable_lamports_in IS NULL
  AND first_slot_buy_lamports IS NULL
  AND first_slot_sell_lamports IS NULL
  AND name ~ ' · bkt=[^ ·]+$';

-- 3. A width below 1e-4 rendered as `bkt=0` under the old fixed-4-decimal name --
--    the one width `validate` rejects, on a row whose width is legal. The name is
--    now rendered at `decimals_for(width)`; restate the rows that carry the lie.
UPDATE fingerprints SET name = regexp_replace(name, ' · bkt=0$', ' · bkt=' || trim(trailing '0' from to_char(bucket_size_amount, 'FM0.999999999999'))), updated_at = now()
WHERE bucket_size_amount IS NOT NULL AND bucket_size_amount < 1e-4 AND name ~ ' · bkt=0$';

-- 4. Merge the rows that are now byte-identical in every match axis.
--
--    Restricted to groups whose `metric_config` is identical as well. That column
--    is NOT match identity, so it does not fork the key -- but it is live (it
--    compiles into `m_flow_split`'s patterns at reload), so collapsing rows that
--    disagree on it would silently retune every rule underneath. The ten
--    `8dtx . <router>` rows are exactly that case: match-identical, ten different
--    pattern sets, and the router split is the whole point of them. They stay.
--
--    Winner: most rules, then oldest. Rules move to it; the losers are deleted.
WITH ident AS (
    SELECT id, name, created_at, metric_config,
           (SELECT count(*) FROM strategy_rules r WHERE r.fingerprint_id = f.id) AS rules,
           CASE WHEN wildcard THEN 'W' ELSE concat_ws('~',
               coalesce(cu_limit::text, '-'), coalesce(cu_price::text, '-'),
               coalesce(array_to_string(ix_labels, ','), '-'),
               coalesce(init_buy_lamports::text, '-'), coalesce(max_cost_lamports::text, '-'),
               coalesce(spendable_lamports_in::text, '-'),
               coalesce(first_slot_buy_lamports::text, '-'),
               coalesce(first_slot_sell_lamports::text, '-'),
               coalesce(bucket_size_amount::text, 'exact')) END
           || '##' || md5(metric_config::text) AS match_key
    FROM fingerprints f
), ranked AS (
    SELECT id, match_key,
           first_value(id) OVER (PARTITION BY match_key ORDER BY rules DESC, created_at) AS winner
    FROM ident
), merged AS (
    SELECT id AS loser, winner FROM ranked WHERE id <> winner
)
UPDATE strategy_rules r SET fingerprint_id = m.winner, updated_at = now()
FROM merged m WHERE r.fingerprint_id = m.loser;

-- The lab's `grouped_sweep_runs.fingerprint_id` is deliberately FK-free (a deleted
-- fingerprint must not delete a run's history), so a merged-away id would dangle
-- there and silently scope a re-run to nothing. Repoint it on the same key.
WITH ident AS (
    SELECT id, created_at, metric_config,
           (SELECT count(*) FROM strategy_rules r WHERE r.fingerprint_id = f.id) AS rules,
           CASE WHEN wildcard THEN 'W' ELSE concat_ws('~',
               coalesce(cu_limit::text, '-'), coalesce(cu_price::text, '-'),
               coalesce(array_to_string(ix_labels, ','), '-'),
               coalesce(init_buy_lamports::text, '-'), coalesce(max_cost_lamports::text, '-'),
               coalesce(spendable_lamports_in::text, '-'),
               coalesce(first_slot_buy_lamports::text, '-'),
               coalesce(first_slot_sell_lamports::text, '-'),
               coalesce(bucket_size_amount::text, 'exact')) END
           || '##' || md5(metric_config::text) AS match_key
    FROM fingerprints f
), ranked AS (
    SELECT id, first_value(id) OVER (PARTITION BY match_key ORDER BY rules DESC, created_at) AS winner
    FROM ident
)
DELETE FROM fingerprints WHERE id IN (SELECT id FROM ranked WHERE id <> winner);

-- 5. The backstop. A width with no SOL axis to spend it on is now unstorable, so
--    the duplicates step 4 cleaned up cannot be created again by a writer that
--    skips `effective_bucket_size_amount`.
ALTER TABLE fingerprints
    DROP CONSTRAINT IF EXISTS fingerprints_bucket_width_needs_a_sol_axis;
ALTER TABLE fingerprints
    ADD CONSTRAINT fingerprints_bucket_width_needs_a_sol_axis CHECK (
        bucket_size_amount IS NULL
        OR init_buy_lamports IS NOT NULL
        OR max_cost_lamports IS NOT NULL
        OR spendable_lamports_in IS NOT NULL
        OR first_slot_buy_lamports IS NOT NULL
        OR first_slot_sell_lamports IS NOT NULL
    );
