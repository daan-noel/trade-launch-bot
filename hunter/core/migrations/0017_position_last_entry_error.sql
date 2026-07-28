-- 0017_position_last_entry_error.sql — make a buy failure's CAUSE durable.
--
-- WHY. An `EntryFailed` row recorded that the buy never filled and nothing else:
-- `reduce.rs` emits the terminal delta with `reason: None` (there is no
-- `ExitReason` for a position that never opened), and the executor threw away the
-- one fact that explains it — the `TradeError` from the send, or the Anchor
-- custom code from the on-chain revert. On 2026-07-27 that cost a log-dig: 9
-- `EntryFailed` rows in an 8 h window, all with zero on-chain buys, and no way to
-- tell a slippage revert (6002/6042 ⇒ the buy floor is too tight, a TUNING fix)
-- from a structural one (a code fix) without pulling container logs off the box.
-- ~27 landed reverts of burnt fees hinge on that distinction.
--
-- SEMANTICS. "The most recent buy attempt that did not fill", regardless of where
-- the row ended up — NOT an `EntryFailed`-only field. A `Holding` row that reads
-- `reverted 6002 (curve buy slippage)` entered on a later attempt, and that is
-- useful history, not stale data; nothing clears it on success.
--
-- Like `exit_redrive_count` / `exit_parked` (0012), this column is deliberately
-- NOT in `StrategyRepo::update_position`'s fixed column list: the executor writes
-- it at the moment of failure and the engine sink's full-row write of the terminal
-- `EntryFailed` status lands afterwards, so a shared write path would clobber it.
-- `note_last_entry_error` is the ONE writer.

ALTER TABLE strategy_positions
    ADD COLUMN IF NOT EXISTS last_entry_error TEXT;

COMMENT ON COLUMN strategy_positions.last_entry_error IS
    'Cause of the most recent buy attempt that did not fill (send error or Anchor '
    'custom code). Written only by note_last_entry_error; never cleared on success.';
