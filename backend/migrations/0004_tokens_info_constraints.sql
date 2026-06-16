-- Restore the PK / UNIQUE that 0001_init declares for `tokens_info`
-- (`id UUID PRIMARY KEY`, `mint_address TEXT UNIQUE NOT NULL`) but which the
-- live table was missing — it had been created/restored with only its NOT NULL
-- constraints (the lingering `tokens_info_is_rugged_not_null` name shows it
-- predates 0002's rename), so it never carried the PK/UNIQUE.
--
-- Without the UNIQUE on `mint_address`, every `ON CONFLICT (mint_address)`
-- upsert — metrics flush, migration flag, sync cursor — fails with
-- "there is no unique or exclusion constraint matching the ON CONFLICT
-- specification". Verified zero duplicate mints / null ids before adding.
--
-- Guarded so a freshly-migrated DB (which already has both from 0001) is a no-op.

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conrelid = 'tokens_info'::regclass AND contype = 'p'
    ) THEN
        ALTER TABLE tokens_info ADD CONSTRAINT tokens_info_pkey PRIMARY KEY (id);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conrelid = 'tokens_info'::regclass
          AND contype = 'u'
          AND conkey = ARRAY[
              (SELECT attnum FROM pg_attribute
               WHERE attrelid = 'tokens_info'::regclass AND attname = 'mint_address')
          ]
    ) THEN
        ALTER TABLE tokens_info ADD CONSTRAINT tokens_info_mint_address_key UNIQUE (mint_address);
    END IF;
END $$;
