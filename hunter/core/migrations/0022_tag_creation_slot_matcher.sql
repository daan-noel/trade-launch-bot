-- A tag reads creation-slot buyers through the `creation_slot` matcher: they carry the
-- tag (the dev's birth bundle), and there is no "neither" half. The tag parser refuses
-- the `exclude_creation_slot` option, so a stored tag still holding it would drop out
-- of compilation and read NaN. Each one becomes the matcher when the option was on and
-- simply loses the key when it was off.

UPDATE fingerprints f
SET tags = (
    SELECT jsonb_object_agg(
        e.name,
        CASE
            WHEN jsonb_typeof(e.def) = 'object' AND e.def ? 'exclude_creation_slot' THEN
                (e.def - 'exclude_creation_slot')
                || CASE
                       WHEN e.def -> 'exclude_creation_slot' = 'true'::jsonb
                       THEN jsonb_build_object('match', COALESCE(e.def -> 'match', '{}'::jsonb)
                                                        || '{"creation_slot": true}'::jsonb)
                       ELSE '{}'::jsonb
                   END
            ELSE e.def
        END)
    FROM jsonb_each(f.tags) AS e(name, def)
)
WHERE jsonb_typeof(f.tags) = 'object'
  AND EXISTS (
      SELECT 1 FROM jsonb_each(f.tags) AS e(name, def)
      WHERE jsonb_typeof(e.def) = 'object' AND e.def ? 'exclude_creation_slot'
  );
