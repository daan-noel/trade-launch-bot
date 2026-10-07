import { useCallback, useMemo, useState } from 'react';
import { skipToken } from '@reduxjs/toolkit/query/react';
import { DataTable } from 'components/table/DataTable';
import { LazyTokenTradeChart } from 'components/tokens/LazyTokenTradeChart';
import { useTokenHighlight } from 'components/tokens/useTokenHighlight';
import { rangeForSpan, rangeSpan, type RangeTapeSpan } from 'components/token-price-chart/barTrades';
import type { ChartEventMarker, ChartRangeControl, ChartRangeSelectionDetail } from 'components/token-price-chart/types';
import { Button } from 'components/ui/Button';
import { StatTile } from 'components/ui/StatTile';
import { useTimezone } from 'context/TimezoneContext';
import { apiErrorMessage } from 'store/apiSlice';
import { useGetTokenDetailQuery, useGetTokenTradesQuery } from 'store/sharedEndpoints';
import { formatTimestampMs } from 'utils/date';
import { AXIS_BY_KEY, formatAxis } from '@lab/lib/entryContext/axes';
import type { EntryRow, EntryTargetTag } from '@lab/lib/entryContext/types';
import type { TradeRecord } from 'types';
import { useGetEntryRangeQuery } from '@lab/store/labEndpoints';
import { GROUP_TABLE_LABELS, groupColumns } from './entryColumns';

const EMPTY_TRADES: TradeRecord[] = [];

/** Chart markers for the two signals on this read. Each pins to the trade at
 *  that `(slot, tx_index)`. */
function signalMarkers(trades: TradeRecord[], entry: EntryRow): ChartEventMarker[] {
  const mark = (slot: number, txIndex: number, label: string): ChartEventMarker | null => {
    const t =
      trades.find((x) => x.slot === slot && x.tx_index === txIndex && x.leg_index === 0) ??
      trades.find((x) => x.slot === slot && x.tx_index === txIndex);
    if (!t) return null;
    return {
      kind: 'entry',
      time: t.block_time,
      priceInSol: t.price_per_token,
      txSignature: t.tx_signature,
      label,
      role: 'signal',
    };
  };
  const out: ChartEventMarker[] = [];
  const ix = entry.ix_pick;
  if (ix) {
    const m = mark(ix.slot, ix.tx_index, 'ix');
    if (m) out.push(m);
  }
  if (entry.reserve) {
    const m = mark(entry.reserve.slot, entry.reserve.tx_index, 'reserve');
    if (m) out.push(m);
  }
  return out;
}
const GROUP_COLS_HIDDEN: Readonly<Record<string, boolean>> = { sell_sol: false, buy_secs: false };
/** The numbers read for the selected range, in this order (defined once, in `axes.ts`). */
const RANGE_AXES = ['tx_share', 'sol_share', 'tag_buy_tx', 'buy_tx', 'ctl_tx_share', 'tx_share_lift'] as const;

/** What the detail reads its range with: the page's committed query. */
export interface EntryDetailQuery {
  wallet: string;
  windowSecs: number;
  probeSlots: number;
  slotsBefore: number;
  slotsAfter: number;
  slippagePct: number[];
  slackLamports: number;
  tag?: EntryTargetTag;
}

/**
 * One buy, opened: the token's chart and the numbers for a range of its tape.
 *
 * The range starts as the analysis window (the `W` seconds before his buy),
 * drawn as the chart's host range (indigo band), and its numbers are the buy
 * row's own. **Pick range** arms the host range's own drag-select: a dragged range
 * is read by the server exactly as an entry window is (`entry-context/range`: the
 * range is the window, the same length before it the control), so every number
 * below means the same thing for any range. **Reset** re-selects the analysis
 * window. The chart's own candle click and range select (teal) are untouched:
 * they list their trades in the table under the chart, as on every chart.
 *
 * The chart is the shared trade chart without its card: his trades spotlit, the
 * lens set classifying the flow. The breakdown's Structure
 * cells carry the trades table's highlight button: it washes every candle with
 * that exact structure.
 */
export function EntryDetail({ entry, query }: { entry: EntryRow; query: EntryDetailQuery }) {
  const { timezone } = useTimezone();
  const mint = entry.mint_address;
  const { data: detail } = useGetTokenDetailQuery(mint);
  // The chart's own trades query: same cache key, no second fetch.
  const { data: trades } = useGetTokenTradesQuery(mint);
  // The chart's highlight, owned here so the breakdown's Structure cells arm it.
  const highlight = useTokenHighlight(trades ?? EMPTY_TRADES, mint);
  const { toggleStructure, structureColor } = highlight;
  const groupCols = useMemo(
    () => groupColumns({ toggle: toggleStructure, colorOf: structureColor }),
    [toggleStructure, structureColor],
  );

  const buySec = Date.parse(entry.at) / 1000;
  const analysis = useMemo(
    () => ({ from: buySec - query.windowSecs, to: buySec }),
    [buySec, query.windowSecs],
  );
  // A dragged range; null = the analysis window (the row's own read).
  const [picked, setPicked] = useState<RangeTapeSpan | null>(null);

  const onHostRangeChange = useCallback(
    (r: ChartRangeSelectionDetail | null) => {
      if (!r || !trades) return setPicked(null);
      // The host range's default is the analysis window: keep the row's read.
      const def = rangeForSpan(trades, analysis, r.groupMode, r.intervalSec);
      if (def && def.lo === Math.min(r.lo, r.hi) && def.hi === Math.max(r.lo, r.hi)) return setPicked(null);
      setPicked(rangeSpan(trades, r));
    },
    [trades, analysis],
  );

  const range = useGetEntryRangeQuery(
    picked
      ? {
          wallet: query.wallet,
          mint,
          from: new Date(picked.from * 1000).toISOString(),
          to: new Date(picked.to * 1000).toISOString(),
          end_slot: picked.endSlot,
          probe_slots: query.probeSlots,
          slots_before: query.slotsBefore,
          slots_after: query.slotsAfter,
          slippage_pct: query.slippagePct,
          slack_lamports: query.slackLamports,
          ...(query.tag ? { tag: query.tag } : {}),
        }
      : skipToken,
  );
  const read = picked ? range.data?.read : entry;
  const spanSecs = picked ? (range.data?.window_secs ?? picked.to - picked.from) : query.windowSecs;
  const shown = picked ?? analysis;

  const toolbarRow = (ctl: ChartRangeControl) => (
    <>
      <span className="text-[10px] font-bold uppercase tracking-widest text-text-dim">Analysis range</span>
      <Button
        size="xs"
        variant="ghost"
        active={ctl.picking}
        onClick={() => ctl.setPicking(!ctl.picking)}
        title="Drag on the chart to pick the analysis range (indigo band). The numbers below then read that range instead of the analysis window. The toolbar's own range select (teal) stays separate: it lists its trades in the table under the chart."
      >
        Pick range
      </Button>
      <Button
        size="xs"
        variant="link"
        disabled={!picked}
        onClick={() => {
          ctl.setPicking(false);
          ctl.selectSpan(analysis);
          setPicked(null);
        }}
        title={`Back to the analysis window: the ${query.windowSecs}s before his buy.`}
      >
        Reset to {query.windowSecs}s before his buy
      </Button>
      <span className="font-mono text-[11px] text-text">
        {formatTimestampMs(shown.from * 1000, timezone)} → {formatTimestampMs(shown.to * 1000, timezone)}
      </span>
      <span className="text-[11px] text-text-dim">
        {spanSecs.toFixed(spanSecs < 10 ? 1 : 0)}s · {picked ? 'picked range' : 'analysis window'} · Earlier = the
        same length just before
      </span>
    </>
  );

  return (
    <div className="flex flex-col gap-2 p-2">
      <LazyTokenTradeChart
        detail={detail ?? null}
        eventMarkers={signalMarkers(trades ?? EMPTY_TRADES, entry)}
        highlightWallet={query.wallet}
        tableId="entry_context_chart"
        toolbarRow={toolbarRow}
        defaultRange={analysis}
        hostRangeLabel="Analysis"
        onHostRangeChange={onHostRangeChange}
        highlight={highlight}
      />

      {picked && range.isFetching && <p className="text-[11px] text-text-dim">Reading the range…</p>}
      {picked && range.error && (
        <p className="text-[11px] text-red">{apiErrorMessage(range.error, 'Failed to read the range')}</p>
      )}
      {picked && !range.isFetching && !range.data && !range.error && (
        <p className="text-[11px] text-text-dim">No trade in the picked range to read.</p>
      )}

      {read && (
        <>
          <div className="grid grid-cols-3 gap-2 lg:grid-cols-6">
            {RANGE_AXES.map((k) => {
              const a = AXIS_BY_KEY.get(k)!;
              return (
                <StatTile
                  key={k}
                  size="sm"
                  label={a.label}
                  value={read.unknown_reason ? '-' : formatAxis(a, a.get(read))}
                  info={
                    picked
                      ? `${a.label}, read over the range you picked. Earlier = the same length just before it.`
                      : a.definition(query.windowSecs)
                  }
                />
              );
            })}
          </div>
          <DataTable
            columns={groupCols}
            rows={read.groups}
            rowKey={(g) => g.key}
            tableId="entry_context_groups"
            defaultCols={GROUP_COLS_HIDDEN}
            groupLabels={GROUP_TABLE_LABELS}
            defaultSort={{ col: 'buy_tx', dir: 'desc' }}
            paginate={false}
            hoverable
            emptyMessage="No other trades in this range"
          />
          {read.groups_omitted > 0 && (
            <p className="text-[11px] text-text-dim">{read.groups_omitted} smaller structures not listed</p>
          )}
        </>
      )}
    </div>
  );
}
