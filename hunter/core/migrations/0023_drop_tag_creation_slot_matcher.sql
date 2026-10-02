-- A tag has no `creation_slot` matcher: the tag parser refuses the key, so a stored tag
-- still holding it would drop out of compilation and read NaN. Each one loses the key; a
-- creation-slot buyer is then the rest unless another matcher names it. A tag whose only
-- matcher was `creation_slot` keeps an empty `ix_shape`, a valid tag that matches nothing.

UPDATE fingerprints f
SET tags = (
    SELECT jsonb_object_agg(
        e.name,
        CASE
            WHEN jsonb_typeof(e.def) = 'object' AND jsonb_typeof(e.def -> 'match') = 'object'
                 AND e.def -> 'match' ? 'creation_slot' THEN
                jsonb_set(
                    e.def,
                    '{match}',
                    CASE
                        WHEN (e.def -> 'match') - 'creation_slot' = '{}'::jsonb THEN '{"ix_shape": []}'::jsonb
                        ELSE (e.def -> 'match') - 'creation_slot'
                    END)
            ELSE e.def
        END)
    FROM jsonb_each(f.tags) AS e(name, def)
)
WHERE jsonb_typeof(f.tags) = 'object'
  AND EXISTS (
      SELECT 1 FROM jsonb_each(f.tags) AS e(name, def)
      WHERE jsonb_typeof(e.def) = 'object' AND jsonb_typeof(e.def -> 'match') = 'object'
        AND e.def -> 'match' ? 'creation_slot'
  );
