import { useEffect } from 'react';
import { useDispatch } from 'react-redux';
import { connectStrategyPositionUpdate } from 'services/sse';
import { liveApi } from '@live/store/liveEndpoints';
import type { AppDispatch } from '@live/store';

/** One refetch per burst — a Stop closes every position of a rule at once, and the
 *  list recomputes every rule's counters server-side. */
const COALESCE_MS = 1_000;

/**
 * The transitions that move a Rules-board counter (`RULE_COUNTERS_AGGS` in
 * `strategy_repo.rs`): `BuySubmitted` opens a pending row, `Holding` makes it
 * entered, `End` / `EntryFailed` close it into W/L + PnL. `ExitPending`,
 * `ExitStuck` and `ExitUnconfirmed` stay in the open partition and move nothing.
 */
const SCORE_CHANGING = new Set(['BuySubmitted', 'Holding', 'End', 'EntryFailed']);

/**
 * Keep the Rules board's per-rule scores (PnL, Return%, Exp, Win%, W/L, N) and the
 * Evidence run chips current. Both are `StrategyRule`-tagged GETs whose counters
 * the server folds from `strategy_positions`, so a position frame invalidates the
 * tag rather than patching a figure the client cannot recompute. The sink sends a
 * frame only after its row commits, so the refetch reads the new counters. A
 * reconnect invalidates too: the gap's frames are gone.
 */
export function useRuleScoresRealtime(): void {
  const dispatch = useDispatch<AppDispatch>();

  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const bump = () => {
      if (timer !== undefined) return;
      timer = setTimeout(() => {
        timer = undefined;
        dispatch(liveApi.util.invalidateTags(['StrategyRule']));
      }, COALESCE_MS);
    };
    const h = connectStrategyPositionUpdate(
      (d) => {
        if (SCORE_CHANGING.has(d.status)) bump();
      },
      bump,
    );
    return () => {
      h.close();
      clearTimeout(timer);
    };
  }, [dispatch]);
}
