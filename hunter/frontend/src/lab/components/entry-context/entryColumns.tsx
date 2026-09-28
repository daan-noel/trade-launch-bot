import type { ColumnDef } from 'components/table/types';
import { DateCell } from 'components/table/DateCell';
import { Badge } from 'components/ui/Badge';
import { IxLabelsDisplay } from 'components/ui/IxLabelsDisplay';
import { ENTRY_AXES, formatAxis } from '@lab/lib/entryContext/axes';
import {
  entryKey,
  UNKNOWN_REASON_TEXT,
  type EntryGroupRow,
  type EntryRow,
} from '@lab/lib/entryContext/types';

/**
 * Entry rows: when he bought, whether the entry clears the filter, then one
 * column per axis in `ENTRY_AXES` — generated, so an axis defined there is a
 * column here with its definition as the header tooltip.
 */
export function entryColumns(
  windowSecs: number,
  passing: ReadonlySet<string>,
): ColumnDef<EntryRow>[] {
  const axisCols: ColumnDef<EntryRow>[] = ENTRY_AXES.map((a) => ({
    key: a.key,
    label: a.label,
    tooltip: a.definition(windowSecs),
    group: 'axes',
    render: (e) => (e.unknown_reason ? <span className="text-text-dim">-</span> : formatAxis(a, a.get(e))),
    sortValue: (e) => (e.unknown_reason ? null : a.get(e)),
    searchValue: () => '',
    filterNumber: (e) => (e.unknown_reason ? null : a.get(e)),
  }));
  return [
    {
      key: 'at',
      label: 'His buy',
      tooltip: 'Block time of his buy transaction (tape order is slot, tx index).',
      render: (e) => <DateCell iso={e.at} />,
      sortValue: (e) => e.slot * 10_000 + e.tx_index,
      searchValue: (e) => e.at,
    },
    {
      key: 'pass',
      label: 'Filter',
      tooltip: 'Whether this entry clears every filter line. Unknown entries never pass.',
      render: (e) =>
        e.unknown_reason ? (
          <span title={`Unknown: ${UNKNOWN_REASON_TEXT[e.unknown_reason]}`}>
            <Badge variant="warning" size="sm">
              Unknown
            </Badge>
          </span>
        ) : passing.has(entryKey(e)) ? (
          <Badge variant="accent" size="sm">
            Pass
          </Badge>
        ) : (
          <Badge variant="neutral" size="sm">
            Out
          </Badge>
        ),
      sortValue: (e) => (e.unknown_reason ? -1 : passing.has(entryKey(e)) ? 1 : 0),
      searchValue: () => '',
      filterOptions: [
        { value: 'pass', label: 'Pass' },
        { value: 'out', label: 'Out' },
        { value: 'unknown', label: 'Unknown' },
      ],
      filterOptionValue: (e) => (e.unknown_reason ? 'unknown' : passing.has(entryKey(e)) ? 'pass' : 'out'),
    },
    ...axisCols,
  ];
}

const pctText = (v: number | null) => (v == null ? '-' : `${v.toFixed(0)}%`);

/** One entry's window broken down by structure. */
export function groupColumns(): ColumnDef<EntryGroupRow>[] {
  const num = (
    key: string,
    label: string,
    tooltip: string,
    get: (g: EntryGroupRow) => number | null,
    fmt: (v: number | null) => string,
  ): ColumnDef<EntryGroupRow> => ({
    key,
    label,
    tooltip,
    render: (g) => fmt(get(g)),
    sortValue: get,
    searchValue: () => '',
    filterNumber: get,
  });
  const sol = (v: number | null) => (v == null ? '-' : v.toFixed(3));
  const int = (v: number | null) => (v == null ? '-' : String(v));
  return [
    {
      key: 'key',
      label: 'Structure',
      width: '420px',
      render: (g) =>
        g.labels ? (
          <IxLabelsDisplay labels={g.labels} maxHeight="4.5rem" copyJson />
        ) : (
          <span className="font-mono text-[11px]">{g.key}</span>
        ),
      searchValue: (g) => g.key,
    },
    {
      key: 'tagged',
      label: 'Tag',
      tooltip: 'Buy transactions of this structure that carried the target tag.',
      render: (g) =>
        g.tag_buy_tx > 0 ? (
          <Badge variant="accent" size="sm">
            {g.tag_buy_tx === g.buy_tx ? 'tag' : `${g.tag_buy_tx}/${g.buy_tx}`}
          </Badge>
        ) : (
          <span className="text-text-dim">-</span>
        ),
      sortValue: (g) => g.tag_buy_tx,
      searchValue: () => '',
    },
    num('buy_tx', 'Buy TXs', 'Buy transactions of this structure in the window.', (g) => g.buy_tx, int),
    num('buy_tx_share', 'Tx %', 'Its buy transactions over every buy transaction in the window.', (g) => g.buy_tx_share_pct, pctText),
    num('buy_sol', 'Buy SOL', 'SOL its buys spent in the window.', (g) => g.buy_sol, sol),
    num('buy_sol_share', 'SOL %', 'Its buy SOL over every buy SOL in the window.', (g) => g.buy_sol_share_pct, pctText),
    num('sell_tx', 'Sell TXs', 'Sell transactions of this structure in the window.', (g) => g.sell_tx, int),
    num('sell_sol', 'Sell SOL', 'SOL its sells took out in the window.', (g) => g.sell_sol, sol),
    num('wallets', 'Wallets', 'Distinct wallets among its prints.', (g) => g.wallets, int),
    num('buy_secs', 'Buy secs', 'Distinct one-second buckets holding one of its buys.', (g) => g.buy_secs, int),
  ];
}
