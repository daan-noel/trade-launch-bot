import { Link } from 'react-router-dom';
import type { ColumnDef } from 'components/table/types';
import type { OpenStrategyPosition } from 'types';
import type { BadgeVariant } from 'components/ui/Badge';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { Badge } from 'components/ui/Badge';
import { legColumns } from 'components/strategy/strategyColumns';

/** Friendly labels for the canonical strategy ids. */
const STRATEGY_LABEL: Record<string, string> = {
  tpsl_sniper_1: 'TPSL1',
  tpsl_sniper_2: 'TPSL2',
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

/** Columns for the cross-strategy open-positions monitor. */
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
      key: 'mint_address',
      label: 'Mint',
      width: '195px',
      sortable: true,
      render: (r) => <AddressDisplay address={r.mint_address} kind="token" truncate={12} stopPropagation />,
      sortValue: (r) => r.mint_address,
      searchValue: (r) => r.mint_address,
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
    ...legColumns<OpenStrategyPosition>(
      'entry',
      {
        price: (r) => r.entry_price ?? null,
        size: (r) => r.entry_sol ?? null,
        time: (r) => r.entry_time ?? null,
      },
      {
        fields: ['price', 'size', 'time'],
        width: { price: '120px', size: '110px', time: '120px' },
      },
    ),
    {
      key: 'trade',
      label: '',
      width: '64px',
      render: (r) => (
        <Link
          to={`/trade?mint=${encodeURIComponent(r.mint_address)}`}
          className="text-[11px] font-semibold text-accent hover:text-primary hover:underline"
          onClick={(e) => e.stopPropagation()}
        >
          Trade
        </Link>
      ),
      searchValue: () => '',
    },
  ];
}
