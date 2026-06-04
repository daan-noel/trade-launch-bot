import type { ColumnDef } from '../table/types';
import type { AnalysisRecord, CreatorRecord } from '../../types';
import { formatIso } from '../../utils/date';
import { formatDecimal } from '../../utils/format';
import { cn } from '../../lib/cn';
import { AddressDisplay } from '../ui/AddressDisplay';

function scoreBadge(score: number) {
  const pct = `${(score * 100).toFixed(0)}%`;
  const cls =
    score >= 0.7 ? 'text-red font-bold' : score >= 0.4 ? 'text-warning font-bold' : 'text-primary font-bold';
  return <span className={cn('font-mono', cls)}>{pct}</span>;
}

export const creatorColumns: ColumnDef<CreatorRecord>[] = [
  {
    key: 'wallet',
    label: 'Wallet',
    render: (c) => (
      <AddressDisplay address={c.wallet_address} kind="account" truncateLen={12} />
    ),
    sortValue: (c) => c.wallet_address,
    searchValue: (c) => c.wallet_address,
  },
  {
    key: 'tokens_created',
    label: 'Tokens Created',
    sortable: true,
    render: (c) => c.tokens_created,
    sortValue: (c) => c.tokens_created,
    searchValue: (c) => String(c.tokens_created),
  },
  {
    key: 'volume',
    label: 'Volume (SOL)',
    sortable: true,
    render: (c) => formatDecimal(c.total_volume_sol, 4),
    sortValue: (c) => c.total_volume_sol,
    searchValue: (c) => String(c.total_volume_sol),
  },
  {
    key: 'suspiciousness',
    label: 'Suspiciousness',
    sortable: true,
    render: (c) => scoreBadge(c.suspiciousness_score),
    sortValue: (c) => c.suspiciousness_score,
    searchValue: (c) => String(c.suspiciousness_score),
  },
  {
    key: 'wash_trade',
    label: 'Wash Trade',
    sortable: true,
    render: (c) => scoreBadge(c.wash_trade_score),
    sortValue: (c) => c.wash_trade_score,
    searchValue: (c) => String(c.wash_trade_score),
  },
  {
    key: 'last_analyzed',
    label: 'Last Analyzed',
    sortable: true,
    render: (c) => (c.last_analyzed_at ? formatIso(c.last_analyzed_at) : '-'),
    sortValue: (c) => c.last_analyzed_at ?? '',
    searchValue: (c) => c.last_analyzed_at ?? '',
  },
];

export const analysisColumns: ColumnDef<AnalysisRecord>[] = [
  {
    key: 'analyzer',
    label: 'Analyzer',
    sortable: true,
    render: (r) => r.analyzer_name,
    sortValue: (r) => r.analyzer_name,
    searchValue: (r) => r.analyzer_name,
  },
  {
    key: 'score',
    label: 'Score',
    sortable: true,
    render: (r) => scoreBadge(r.score),
    sortValue: (r) => r.score,
    searchValue: (r) => String(r.score),
  },
  {
    key: 'indicators',
    label: 'Indicators',
    render: (r) => {
      const text = r.indicators.length ? r.indicators.join(' · ') : '-';
      return (
        <span title={text} className="block max-w-[320px] truncate text-[11px] text-text-dim">
          {text}
        </span>
      );
    },
    searchValue: (r) => r.indicators.join(' '),
  },
  {
    key: 'computed_at',
    label: 'Computed At',
    sortable: true,
    render: (r) => formatIso(r.computed_at),
    sortValue: (r) => r.computed_at,
    searchValue: (r) => r.computed_at,
  },
];
