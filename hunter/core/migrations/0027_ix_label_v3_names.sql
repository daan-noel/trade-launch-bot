-- Rename instruction labels that were stored as `ix#<discriminator>` before
-- the names existed. The bytes are `sha256("global:<name>")[..8]`.
--
-- Pump.Fun:  buy_v3, sell_v3, buy_exact_quote_in_v3
-- PumpSwap:  buy_v2, sell_v2, buy_exact_quote_in_v2
--
-- `trades` is compressed after 7 days. An UPDATE on the hypertable plans every
-- chunk, and a compressed chunk then decompresses under a hard tuple cap. The
-- rewrite below names each uncompressed chunk table. A compressed chunk holds
-- prints from before these instructions were in the label table only when the
-- upgrade is older than the compression horizon.
--
-- The hot pool sets statement_timeout to 60s. This file is one transaction, and
-- the chunk rewrite is one statement longer than that. The override lasts for
-- this transaction only.
SET LOCAL statement_timeout = 0;

CREATE OR REPLACE FUNCTION hunter_rename_ix_label(label text) RETURNS text
LANGUAGE sql IMMUTABLE AS $$
    SELECT CASE label
        WHEN 'Pump.Fun: ix#07051dc4f5176550' THEN 'Pump.Fun: BuyV3'
        WHEN 'Pump.Fun: ix#1c92de7726c469d5' THEN 'Pump.Fun: SellV3'
        WHEN 'Pump.Fun: ix#e1f7501ed5b38488' THEN 'Pump.Fun: BuyExactQuoteInV3'
        WHEN 'PumpSwap: ix#b817ee6167c5d33d' THEN 'PumpSwap: BuyV2'
        WHEN 'PumpSwap: ix#5df6823ce7e940b2' THEN 'PumpSwap: SellV2'
        WHEN 'PumpSwap: ix#c2ab1c46684d5b2f' THEN 'PumpSwap: BuyExactQuoteInV2'
        ELSE label
    END
$$;

CREATE OR REPLACE FUNCTION hunter_rename_ix_labels(doc jsonb) RETURNS jsonb
LANGUAGE plpgsql IMMUTABLE AS $$
DECLARE
    arr jsonb;
    renamed jsonb;
BEGIN
    IF doc IS NULL THEN
        RETURN doc;
    ELSIF jsonb_typeof(doc) = 'array' THEN
        arr := doc;
    ELSIF jsonb_typeof(doc->'instructions') = 'array' THEN
        arr := doc->'instructions';
    ELSE
        RETURN doc;
    END IF;

    SELECT COALESCE(jsonb_agg(to_jsonb(hunter_rename_ix_label(e)) ORDER BY ord), '[]'::jsonb)
      INTO renamed
    FROM jsonb_array_elements_text(arr) WITH ORDINALITY AS x(e, ord);

    IF jsonb_typeof(doc) = 'array' THEN
        RETURN renamed;
    END IF;
    RETURN jsonb_set(doc, '{instructions}', renamed);
END
$$;

UPDATE tokens
   SET ix_labels = hunter_rename_ix_labels(ix_labels)
 WHERE ix_labels::text LIKE '%ix#07051dc4f5176550%'
    OR ix_labels::text LIKE '%ix#1c92de7726c469d5%'
    OR ix_labels::text LIKE '%ix#e1f7501ed5b38488%'
    OR ix_labels::text LIKE '%PumpSwap: ix#b817ee6167c5d33d%'
    OR ix_labels::text LIKE '%PumpSwap: ix#5df6823ce7e940b2%'
    OR ix_labels::text LIKE '%PumpSwap: ix#c2ab1c46684d5b2f%';

UPDATE launch_build_day_stats
   SET ix_labels = hunter_rename_ix_labels(ix_labels)
 WHERE ix_labels::text LIKE '%ix#07051dc4f5176550%'
    OR ix_labels::text LIKE '%ix#1c92de7726c469d5%'
    OR ix_labels::text LIKE '%ix#e1f7501ed5b38488%'
    OR ix_labels::text LIKE '%PumpSwap: ix#b817ee6167c5d33d%'
    OR ix_labels::text LIKE '%PumpSwap: ix#5df6823ce7e940b2%'
    OR ix_labels::text LIKE '%PumpSwap: ix#c2ab1c46684d5b2f%';

UPDATE build_breadth_day_stats
   SET recipe = hunter_rename_ix_labels(recipe)
 WHERE recipe::text LIKE '%ix#07051dc4f5176550%'
    OR recipe::text LIKE '%ix#1c92de7726c469d5%'
    OR recipe::text LIKE '%ix#e1f7501ed5b38488%'
    OR recipe::text LIKE '%PumpSwap: ix#b817ee6167c5d33d%'
    OR recipe::text LIKE '%PumpSwap: ix#5df6823ce7e940b2%'
    OR recipe::text LIKE '%PumpSwap: ix#c2ab1c46684d5b2f%';

UPDATE fingerprints
   SET criteria = replace(replace(replace(replace(replace(replace(
           criteria::text,
           'Pump.Fun: ix#07051dc4f5176550', 'Pump.Fun: BuyV3'),
           'Pump.Fun: ix#1c92de7726c469d5', 'Pump.Fun: SellV3'),
           'Pump.Fun: ix#e1f7501ed5b38488', 'Pump.Fun: BuyExactQuoteInV3'),
           'PumpSwap: ix#b817ee6167c5d33d', 'PumpSwap: BuyV2'),
           'PumpSwap: ix#5df6823ce7e940b2', 'PumpSwap: SellV2'),
           'PumpSwap: ix#c2ab1c46684d5b2f', 'PumpSwap: BuyExactQuoteInV2')::jsonb
 WHERE criteria::text LIKE '%ix#07051dc4f5176550%'
    OR criteria::text LIKE '%ix#1c92de7726c469d5%'
    OR criteria::text LIKE '%ix#e1f7501ed5b38488%'
    OR criteria::text LIKE '%PumpSwap: ix#b817ee6167c5d33d%'
    OR criteria::text LIKE '%PumpSwap: ix#5df6823ce7e940b2%'
    OR criteria::text LIKE '%PumpSwap: ix#c2ab1c46684d5b2f%';

DO $$
DECLARE
    rec record;
BEGIN
    FOR rec IN
        SELECT chunk_schema, chunk_name
        FROM timescaledb_information.chunks
        WHERE hypertable_name = 'trades'
          AND NOT is_compressed
    LOOP
        EXECUTE format(
            'UPDATE %I.%I
                SET ix_labels = hunter_rename_ix_labels(ix_labels)
              WHERE ix_labels::text LIKE %L
                 OR ix_labels::text LIKE %L
                 OR ix_labels::text LIKE %L
                 OR ix_labels::text LIKE %L
                 OR ix_labels::text LIKE %L
                 OR ix_labels::text LIKE %L',
            rec.chunk_schema,
            rec.chunk_name,
            '%ix#07051dc4f5176550%',
            '%ix#1c92de7726c469d5%',
            '%ix#e1f7501ed5b38488%',
            '%PumpSwap: ix#b817ee6167c5d33d%',
            '%PumpSwap: ix#5df6823ce7e940b2%',
            '%PumpSwap: ix#c2ab1c46684d5b2f%'
        );
    END LOOP;
END $$;

DROP FUNCTION hunter_rename_ix_labels(jsonb);
DROP FUNCTION hunter_rename_ix_label(text);
