// Canonical `InspectTarget` shape + the record→target mappers shared by every
// strategy page (tpsl1 / tpsl2 / swing1, live + lab). Previously the interface was
// duplicated in both TokenInspectModal forks and the mappers were copy-pasted into
// all five page files; this is the single source.
import type { RulePositionRecord, SimulatedTokenResult } from 'types';

/** What the token-inspect chart modal needs to draw entry/exit markers for a run. */
export interface InspectTarget {
  mint: string;
  symbol?: string | null;
  entryTime: string | null;
  /** null for an armed position whose entry hasn't filled yet. */
  entryPrice: number | null;
  entryTx?: string | null;
  exitTime: string | null;
  exitPrice: number | null;
  exitTx?: string | null;
  /** Exit reason / position status (e.g. "TakeProfit"); appended to the exit label. */
  exitLabel?: string | null;
}

/** Map a backtest/simulate result row to an inspect target. */
export function inspectFromSim(r: SimulatedTokenResult): InspectTarget {
  return {
    mint: r.mint,
    symbol: r.symbol,
    entryTime: r.entry_time,
    entryPrice: r.entry_price,
    entryTx: r.entry_tx,
    exitTime: r.exit_time,
    exitPrice: r.exit_price,
    exitTx: r.exit_tx,
    exitLabel: r.exit_reason && r.exit_reason !== 'Open' ? r.exit_reason : null,
  };
}

/** Map a live/paper position row to an inspect target. */
export function inspectFromPosition(r: RulePositionRecord): InspectTarget {
  return {
    mint: r.mint,
    entryTime: r.entry_time,
    entryPrice: r.entry_price,
    entryTx: r.entry_tx,
    exitTime: r.exit_time,
    exitPrice: r.exit_price,
    exitTx: r.exit_tx,
    exitLabel: r.status && r.status !== 'Open' ? r.status : null,
  };
}
