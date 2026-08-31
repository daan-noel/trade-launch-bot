-- A trade's `wallet_id` is who the VENUE credited, which is not always a trader.
--
-- pump.fun emits `TradeEvent.user` and we store it. An aggregator does not pass
-- its customer through to pump.fun: it buys as a PDA of its own, then forwards
-- the tokens and refunds the change. The program therefore names the ROUTER, and
-- every customer of that router collapses onto one `wallet_id`.
--
-- Measured on this database, one such account — `ARu4n5mFdZogZAravu7CcizaojWnS6oqka37gdLT5SZn`,
-- a PDA of the OKX DEX Router (`proVF4pMXVaYqmy4NjniPh4pqKNfMmsihgd4wdkCX3u`) — is
-- the single busiest "wallet" on the tape, ahead of every real trader. It is not
-- a trader at all. 930 of `wallet_dict`'s addresses are off-curve, so no keypair
-- can ever sign for them; 814 of those carry trades, 462,483 rows in total, and
-- that one PDA is 427,767 of them.
--
-- The damage runs both ways: a per-wallet aggregate reads N unrelated people as
-- one mega-trader, and a unique-wallet breadth count reads those same N people as
-- one participant. Neither is a rounding error at this share of the tape.
--
-- THE DISCRIMINATOR IS SIGNATURES, not a name list. pump.fun's `buy` requires
-- `user` to sign, so a `user` that signed nothing on its own transaction can only
-- be a PDA that signed a CPI. That test is exact, needs no maintained registry of
-- router addresses, and catches a router the day it deploys.
--
-- FORWARD-ONLY for `trades`. `raw_txs` is opt-in with 3-day retention, so rows
-- written before this migration have no payload to re-decode: their payer is gone
-- for good. NULL/empty means "not captured" and must never be read as "the payer
-- is the wallet".

ALTER TABLE trades
    -- The transaction's FEE PAYER, interned into `wallet_dict` exactly like
    -- `wallet_id`. `account_keys[0]`, which the message format defines as the
    -- first required signer.
    --
    -- Per-TRANSACTION on a per-LEG table, denormalized onto every leg like
    -- `fee_lamports` (see 0013): collapse by `tx_signature` before counting.
    --
    -- This does NOT replace `wallet_id`, and attribution is not simply "use the
    -- payer". A bot can pay from one keypair and trade from another, so the payer
    -- is the right answer precisely when `is_proxied` says the wallet is a
    -- program, and only a candidate otherwise. NULL = written before this
    -- migration.
    ADD COLUMN IF NOT EXISTS payer_id   INTEGER,

    -- TRUE when `wallet_id`'s address is absent from
    -- `account_keys[..num_required_signatures]` — the venue's actor put no
    -- signature on this transaction, so it is a router's proxy PDA.
    --
    -- THREE STATES, and NULL is not FALSE:
    --   NULL   not captured — a pre-0014 row, or a frame that carried no message
    --          header to read the signer count from (an RPC `jsonParsed` payload
    --          without per-key flags). Unknown, not "signed".
    --   FALSE  the wallet signed. Attribute the trade to it.
    --   TRUE   the wallet signed nothing. It is a program; the trader is behind
    --          `payer_id`. EXCLUDE from any per-wallet aggregate.
    ADD COLUMN IF NOT EXISTS is_proxied BOOLEAN;

-- Reading the payer back means a second join to `wallet_dict` on a column the
-- trade-history queries filter by mint/time first, so no index is added here: the
-- join is by primary key on the dictionary side, and `payer_id` is not a search
-- key. Add one only when a query actually scans by payer.

-- ── The dictionary learns which of its entries are not people ────────────────
--
-- `is_proxied` fixes attribution going forward. It cannot fix the 462,483 rows
-- already written, so the ban has to live on the dictionary, where it applies to
-- history too.
ALTER TABLE wallet_dict
    -- TRUE when this address cannot be a trader. Set from the on-curve test: an
    -- Ed25519 public key is a point on the curve, so an address that is NOT is a
    -- program-derived address and no keypair exists that can sign for it.
    --
    -- The test is a property of the 32 bytes, not of anything we observed, which
    -- is why it can be applied to rows whose transactions are long gone.
    --
    -- Not every PDA here is a router proxy — some are protocol vaults or pool
    -- authorities with genuine on-chain activity. None of them is a person, so
    -- the exclusion is correct for wallet-level study either way.
    ADD COLUMN IF NOT EXISTS is_proxy BOOLEAN NOT NULL DEFAULT FALSE;

-- Live ingest keeps this current without a curve check: a wallet observed signing
-- nothing on its own transaction is a proxy by the same argument.
UPDATE wallet_dict w
   SET is_proxy = TRUE
  FROM trades t
 WHERE t.wallet_id = w.id
   AND t.is_proxied
   AND NOT w.is_proxy;

CREATE INDEX IF NOT EXISTS idx_wallet_dict_is_proxy
    ON wallet_dict (id) WHERE is_proxy;
