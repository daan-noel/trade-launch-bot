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
 *  endpoint returns raw strategy positions with no token enrichment). The entry
 *  leg is built by the shared `legColumns` builder — the same one that emits the
 *  entry columns in the TPSL/Swing Positions and Sim tables — so the leg's
 *  Price/Size/Time cells stay identical across every position table. This record
 *  carries only an entry leg (no target/exit, no per-fill token count or tx), and
 *  `entry_sol` is a real SOL field rather than a derived price × tokens. */
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
      render: (r) => <AddressDisplay address={r.mint_address} kind="token" truncateLen={12} stopPropagation />,
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
  ];
}
