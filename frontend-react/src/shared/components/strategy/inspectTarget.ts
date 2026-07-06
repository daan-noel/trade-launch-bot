// Canonical `InspectTarget` shape + the record→target mappers shared by every
// strategy page (tpsl1 / tpsl2 / swing1, live + lab). Previously the interface was
// duplicated in both TokenInspectModal forks and the mappers were copy-pasted into
// all five page files; this is the single source.
import type {
  ChartEventMarker,
  ChartSwingLeg,
  ChartSwingOverlay,
} from 'components/token-price-chart';
import type { ChartOverlayHook } from 'components/tokens/TokenChartsGrid';
import type { RulePositionRecord, SimulatedTokenResult } from 'types';

/** What the token-inspect chart modal needs to draw entry/exit markers for a run. */
export interface InspectTarget {
  mint_address: string;
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

/** Build the chart entry/exit markers for an inspect target. Shared by both the
 *  inspect modals and the per-row charts-grid overlay, so the "chart" button's
 *  inline charts mark entry/exit identically to the modal. */
export function buildEventMarkers(target: InspectTarget): ChartEventMarker[] {
  const markers: ChartEventMarker[] = [];
  if (target.entryTime != null && target.entryPrice != null) {
    markers.push({
      kind: 'entry',
      time: target.entryTime,
      priceInSol: target.entryPrice,
      txSignature: target.entryTx ?? null,
      label: 'Entry',
    });
  }
  if (target.exitTime != null && target.exitPrice != null) {
    markers.push({
      kind: 'exit',
      time: target.exitTime,
      priceInSol: target.exitPrice,
      txSignature: target.exitTx ?? null,
      label: target.exitLabel ? `Exit · ${target.exitLabel}` : 'Exit',
    });
  }
  return markers;
}

/** Shape swing-detection legs into the chart overlay (perLeg, full-span). Shared
 *  so carried-legs call sites (live positions, sim result rows) don't re-derive it. */
export function swingOverlayFromLegs(legs?: ChartSwingLeg[] | null): ChartSwingOverlay | null {
  return legs && legs.length ? { legs, segmentMode: 'perLeg', perLegFullSpanEnd: true } : null;
}

/** A charts-grid overlay hook that draws only entry/exit markers (no swing legs) —
 *  the tpsl case. Derives from row data, so it calls no hooks and is trivially safe
 *  to invoke per card. */
export function markerRowOverlay<R>(toTarget: (r: R) => InspectTarget): ChartOverlayHook<R> {
  return (row) => ({ eventMarkers: buildEventMarkers(toTarget(row)) });
}

/** A charts-grid overlay hook drawing entry/exit markers + a swing overlay from
 *  legs already carried on the row (no fetch) — the `live` swing1 positions case
 *  and any sim row that carries its own legs. Calls no hooks. */
export function carriedSwingRowOverlay<R>(
  toTarget: (r: R) => InspectTarget,
  legsOf: (r: R) => ChartSwingLeg[] | null | undefined,
): ChartOverlayHook<R> {
  return (row) => ({
    eventMarkers: buildEventMarkers(toTarget(row)),
    swingOverlay: swingOverlayFromLegs(legsOf(row)),
  });
}

/** Map a backtest/simulate result row to an inspect target. */
export function inspectFromSim(r: SimulatedTokenResult): InspectTarget {
  return {
    mint_address: r.mint_address,
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
    mint_address: r.mint_address,
    entryTime: r.entry_time,
    entryPrice: r.entry_price,
    entryTx: r.entry_tx,
    exitTime: r.exit_time,
    exitPrice: r.exit_price,
    exitTx: r.exit_tx,
    exitLabel: r.status && r.status !== 'Open' ? r.status : null,
  };
}
