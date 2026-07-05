import type { ColumnDef } from 'components/table/types';
import type { OpenStrategyPosition } from 'types';
import type { BadgeVariant } from 'components/ui/Badge';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { Badge } from 'components/ui/Badge';
import { DateCell } from 'components/table/DateCell';
import { formatCompact, formatPrice } from 'utils/format';

/** Friendly labels for the canonical strategy ids. */
const STRATEGY_LABEL: Record<string, string> = {
  tpsl_sniper_1: 'TPSL1',
  tpsl_sniper_2: 'TPSL2',
  swing1: 'Swing1',
};

/** Open-position status → badge tone. `ExitPending` is amber (sell in flight). */
function statusVariant(status: string): BadgeVariant {
  switch (status) {
    case 'ExitPending':
      return 'warning';
    case 'Holding':
      return 'primary';
    case 'BuySubmitted':
      return 'info';
    default:
      return 'neutral';
  }
}

/** Columns for the cross-strategy open-positions monitor. Lean (the positions
 *  endpoint returns raw strategy positions with no token enrichment). */
export function positionColumns(): ColumnDef<OpenStrategyPosition>[] {
  return [
    {
      key: 'strategy_id',
      label: 'Strategy',
      width: '90px',
      sortable: true,
      render: (r) => (
        <span className="font-bold text-text">{STRATEGY_LABEL[r.strategy_id] ?? r.strategy_id}</span>
      ),
      sortValue: (r) => r.strategy_id,
      searchValue: (r) => `${r.strategy_id} ${STRATEGY_LABEL[r.strategy_id] ?? ''}`,
    },
    {
      key: 'mint',
      label: 'Mint',
      width: '195px',
      sortable: true,
      render: (r) => <AddressDisplay address={r.mint} kind="token" truncateLen={12} stopPropagation />,
      sortValue: (r) => r.mint,
      searchValue: (r) => r.mint,
    },
    {
      key: 'status',
      label: 'Status',
      width: '110px',
      sortable: true,
      render: (r) => <Badge variant={statusVariant(r.status)}>{r.status}</Badge>,
      sortValue: (r) => r.status,
      searchValue: (r) => r.status,
    },
    {
      key: 'entry_price',
      label: 'Entry Price',
      width: '120px',
      sortable: true,
      render: (r) =>
        r.entry_price != null ? (
          <span className="tabular-nums text-text-mid">◎{formatPrice(r.entry_price)}</span>
        ) : (
          <span className="text-text-dim">—</span>
        ),
      sortValue: (r) => r.entry_price ?? 0,
      searchValue: (r) => String(r.entry_price ?? ''),
    },
    {
      key: 'entry_sol',
      label: 'Entry SOL',
      width: '110px',
      sortable: true,
      render: (r) =>
        r.entry_sol != null ? (
          <span className="tabular-nums text-text">◎{formatCompact(r.entry_sol, 3)}</span>
        ) : (
          <span className="text-text-dim">—</span>
        ),
      sortValue: (r) => r.entry_sol ?? 0,
      searchValue: (r) => String(r.entry_sol ?? ''),
    },
    {
      key: 'entry_time',
      label: 'Entered',
      width: '120px',
      sortable: true,
      render: (r) => (r.entry_time ? <DateCell iso={r.entry_time} /> : <span className="text-text-dim">—</span>),
      sortValue: (r) => r.entry_time ?? '',
      searchValue: (r) => r.entry_time ?? '',
    },
  ];
}
