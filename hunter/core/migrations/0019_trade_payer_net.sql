-- A trade carries what its transaction actually moved in the payer's wallet.
--
-- `amount_lamports` is the venue's own figure: on the curve it is the SOL that
-- reached (or left) the curve, before the protocol and creator fees and without
-- the network fee, the priority fee or the tip. It is the right number to price a
-- print and the wrong one to book a position: a real round trip priced off it
-- read -7.45 % where the wallet lost -10.62 % (34 bot round trips, 2026-09-13/14,
-- checked against the chain).
--
-- This column is the payer's side: `post_balance - pre_balance` of
-- `account_keys[0]` over the whole transaction, SIGNED LAMPORTS (negative = the
-- transaction took SOL from the payer), every fee, tip and venue charge included.
-- SOL the payer moves into or out of token accounts IT OWNS counts as still the
-- payer's, so the rent a buy parks in its own fresh token account (and the refund
-- a close returns) is a deposit, not a spend.
--
-- ATTRIBUTION - per-TRANSACTION on a per-LEG table, denormalized onto every leg
-- exactly like `fee_lamports` and `tip_lamports` (see 0013): collapse by
-- signature before summing.
--
-- FORWARD-ONLY. NULL = written before this migration, or a source that carried no
-- balances. Never coalesce to 0.

ALTER TABLE trades
    ADD COLUMN IF NOT EXISTS payer_net_lamports BIGINT;
