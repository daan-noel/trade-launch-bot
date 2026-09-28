import { useMemo, useState } from 'react';
import { DataTable } from 'components/table/DataTable';
import { LazyTokenTradeChart } from 'components/tokens/LazyTokenTradeChart';
import type { ChartTimeBand } from 'components/token-price-chart/types';
import { useTimezone } from 'context/TimezoneContext';
import { useGetTokenDetailQuery, useGetTokenTradesQuery } from 'store/sharedEndpoints';
import { formatIso } from 'utils/date';
import { entryKey, type EntryRow } from '@lab/lib/entryContext/types';
import { entryColumns, groupColumns } from './entryColumns';

const GROUP_COLUMNS = groupColumns();
const EMPTY_TRADES: never[] = [];

/**
 * One token's detail: the chart with every entry's window (and its control) drawn
 * as bottom lanes, his entries on this token as rows, and the picked entry's
 * window broken down by structure. The picked window's trades open in the chart's
 * own trades panel, his excluded, so the list matches the numbers above it.
 */
export function EntryTokenDetail({
  mint,
  entries,
  passing,
  wallet,
  windowSecs,
  flowPatternKeys,
}: {
  mint: string;
  entries: EntryRow[];
  passing: ReadonlySet<string>;
  wallet: string;
  windowSecs: number;
  flowPatternKeys: ReadonlySet<string> | null;
}) {
  const { timezone } = useTimezone();
  const { data: detail } = useGetTokenDetailQuery(mint);
  // Same cache key the chart reads, so this costs no second fetch.
  const { data: trades = EMPTY_TRADES } = useGetTokenTradesQuery(mint);
  // Opens on the first passing entry, else the first entry. The parent keys this
  // component by mint, so a new token starts here again.
  const [picked, setPicked] = useState<string | null>(() => {
    const first = entries.find((e) => passing.has(entryKey(e))) ?? entries[0];
    return first ? entryKey(first) : null;
  });

  const entry = entries.find((e) => entryKey(e) === picked) ?? null;
  const columns = useMemo(() => entryColumns(windowSecs, passing), [windowSecs, passing]);

  const bands = useMemo<ChartTimeBand[]>(() => {
    const secs = (e: EntryRow) => Date.parse(e.at) / 1000;
    return [
      {
        key: 'window',
        label: `${windowSecs}s`,
        color: 'var(--color-primary)',
        spans: entries.map((e) => ({ from: secs(e) - windowSecs, to: secs(e) })),
      },
      {
        key: 'control',
        label: 'ctl',
        color: 'rgba(255,255,255,0.3)',
        spans: entries.map((e) => ({ from: secs(e) - 2 * windowSecs, to: secs(e) - windowSecs })),
      },
    ];
  }, [entries, windowSecs]);

  // The picked window's trades in tape order, his excluded: `[at - W, at]` by
  // block time and strictly ahead of his transaction — the fold's own cut.
  const selection = useMemo(() => {
    if (!entry) return null;
    const hi = Date.parse(entry.at);
    const lo = hi - windowSecs * 1000;
    const inWindow = trades.filter((t) => {
      const at = Date.parse(t.block_time);
      return (
        t.wallet_address !== wallet &&
        at >= lo &&
        (t.slot < entry.slot || (t.slot === entry.slot && t.tx_index < entry.tx_index))
      );
    });
    return {
      key: entryKey(entry),
      label: `${windowSecs}s before his buy`,
      timeLabel: formatIso(entry.at, timezone),
      trades: inWindow,
      emptyMessage: 'No other trades in this window',
      onClear: () => setPicked(null),
    };
  }, [entry, trades, wallet, windowSecs, timezone]);

  return (
    <section className="mt-3 flex flex-col gap-3 rounded-md border border-white/8 p-3">
      <LazyTokenTradeChart
        key={mint}
        detail={detail ?? null}
        highlightWallet={wallet}
        flowPatternKeys={flowPatternKeys}
        timeBands={bands}
        externalSelection={selection}
        tableId="entry_context_window_trades"
      />
      <h3 className="text-xs font-bold uppercase tracking-widest text-text-dim">
        His buys on this token ({entries.length})
      </h3>
      <DataTable
        columns={columns}
        rows={entries}
        rowKey={entryKey}
        selectedKey={picked}
        onSelect={setPicked}
        tableId="entry_context_token_entries"
        defaultSort={{ col: 'at', dir: 'desc' }}
        defaultPageSize={10}
        colFilters
        colToggle
        hoverable
        emptyMessage="No buys on this token"
      />
      {entry && (
        <>
          <h3 className="text-xs font-bold uppercase tracking-widest text-text-dim">
            Structures in the {windowSecs}s before {formatIso(entry.at, timezone)}
            {entry.groups_omitted > 0 && ` (${entry.groups_omitted} smaller not listed)`}
          </h3>
          <DataTable
            columns={GROUP_COLUMNS}
            rows={entry.groups}
            rowKey={(g) => g.key}
            tableId="entry_context_groups"
            defaultSort={{ col: 'buy_tx', dir: 'desc' }}
            paginate={false}
            colFilters
            hoverable
            emptyMessage="No other trades in this window"
          />
        </>
      )}
    </section>
  );
}
