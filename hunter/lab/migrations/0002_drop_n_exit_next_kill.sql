-- Retire the swing1-only NextKill exit counter. The named tpsl/swing stack is
-- gone and nothing emits that exit reason anymore; keep any historical
-- `strategy_positions.exit_reason = 'NextKill'` strings as opaque labels (they
-- roll into the UI "Other" bucket) rather than a first-class counter.
ALTER TABLE grouped_sweep_results DROP COLUMN IF EXISTS n_exit_next_kill;
