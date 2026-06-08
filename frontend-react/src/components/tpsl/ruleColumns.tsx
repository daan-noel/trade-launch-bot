import type { ColumnDef } from 'components/table/types';
import type { RuleRecord } from 'types';
import { dashF, dashNum, dashPercent } from './utils';
import { formatAge } from 'utils/format';
import { cn } from 'lib/cn';
import { Badge } from 'components/ui/Badge';

export function ruleColumns(onToggleActive: (rule: RuleRecord) => void): ColumnDef<RuleRecord>[] {
  return [
    {
      key: 'name',
      label: 'Name',
      group: 'identity',
      sortable: true,
      render: (r) => <span className="font-semibold font-mono">{r.rule_name}</span>,
      sortValue: (r) => r.rule_name,
      searchValue: (r) => r.rule_name,
    },
    {
      key: 'init_buy',
      label: 'Init Buy',
      group: 'token_fingerprint',
      sortable: true,
      render: (r) => dashF(r.p_initial_buy_sol ?? 0, 15),
      sortValue: (r) => r.p_initial_buy_sol ?? 0,
      searchValue: (r) => String(r.p_initial_buy_sol ?? ''),
    },
    {
      key: 'cu_limit',
      label: 'CU Lim',
      group: 'token_fingerprint',
      sortable: true,
      render: (r) => dashNum(r.p_cu_limit),
      sortValue: (r) => r.p_cu_limit,
      searchValue: (r) => String(r.p_cu_limit ?? ''),
    },
    {
      key: 'cu_price',
      label: 'CU Price',
      group: 'token_fingerprint',
      sortable: true,
      render: (r) => dashNum(r.p_cu_price),
      sortValue: (r) => r.p_cu_price,
      searchValue: (r) => String(r.p_cu_price ?? ''),
    },
    {
      key: 'max_sol',
      label: 'Max SOL',
      group: 'token_fingerprint',
      sortable: true,
      render: (r) => dashF(r.p_max_sol_cost ?? 0, 3),
      sortValue: (r) => r.p_max_sol_cost ?? 0,
      searchValue: (r) => String(r.p_max_sol_cost ?? ''),
    },
    {
      key: 'spendable',
      label: 'Spendable',
      group: 'token_fingerprint',
      sortable: true,
      render: (r) => dashF(r.p_spendable_sol_in ?? 0, 3),
      sortValue: (r) => r.p_spendable_sol_in ?? 0,
      searchValue: (r) => String(r.p_spendable_sol_in ?? ''),
    },
    {
      key: 'labels',
      label: 'Labels',
      group: 'token_fingerprint',
      sortable: true,
      render: (r) => {
        const arr = Array.isArray(r.p_ix_labels) ? r.p_ix_labels : [];
        const tooltip = arr.map(String).join('\n');
        return (
          <span title={tooltip} className="font-mono">
            {arr.length > 0 ? arr.length : '-'}
          </span>
        );
      },
      sortValue: (r) => (Array.isArray(r.p_ix_labels) ? r.p_ix_labels.length : 0),
      searchValue: (r) =>
        Array.isArray(r.p_ix_labels) ? r.p_ix_labels.map(String).join(' ') : '',
    },
    {
      key: 'max_concurrent_tokens',
      label: 'Max Concurrent Tokens',
      group: 'max_concurrency_and_total',
      width: '120px',
      sortable: true,
      render: (r) => dashNum(r.p_max_concurrent_tokens),
      sortValue: (r) => r.p_max_concurrent_tokens,
      searchValue: (r) => String(r.p_max_concurrent_tokens ?? ''),
    },
    {
      key: 'max_total_tokens',
      label: 'Max Total Tokens',
      group: 'max_concurrency_and_total',
      width: '120px',
      sortable: true,
      render: (r) => dashNum(r.p_max_total_tokens),
      sortValue: (r) => r.p_max_total_tokens,
      searchValue: (r) => String(r.p_max_total_tokens ?? ''),
    },
    {
      key: 'buy_amt',
      label: 'Buy Amt',
      tooltip: 'Position size — SOL allocated per buy (paper or real).',
      group: 'rule_param',
      sortable: true,
      render: (r) => dashF(r.buy_amount, 15),
      sortValue: (r) => r.buy_amount,
      searchValue: (r) => String(r.buy_amount),
    },
    {
      key: 'tp',
      label: 'TP',
      tooltip: 'Take profit (%) — exit once price rises this far above the entry price.',
      group: 'rule_param',
      sortable: true,
      render: (r) => <span className="font-bold text-green">{dashPercent(r.take_profit)}</span>,
      sortValue: (r) => r.take_profit,
      searchValue: (r) => String(r.take_profit),
    },
    {
      key: 'sl',
      label: 'SL',
      tooltip: 'Stop loss (%) — exit once price falls this far below the entry price.',
      group: 'rule_param',
      sortable: true,
      render: (r) => <span className="font-bold text-red">{dashPercent(r.stop_loss)}</span>,
      sortValue: (r) => r.stop_loss,
      searchValue: (r) => String(r.stop_loss),
    },
    {
      key: 'trail',
      label: 'Trail',
      tooltip:
        'Trailing stop (%) — exit when price falls this far below the peak reached since entry, banking a reversal. Blank/0 = off.',
      group: 'rule_param',
      sortable: true,
      render: (r) => (
        <span className="font-bold text-warning">{dashPercent(r.p_trailing_stop_pct ?? 0)}</span>
      ),
      sortValue: (r) => r.p_trailing_stop_pct ?? 0,
      searchValue: (r) => String(r.p_trailing_stop_pct ?? ''),
    },
    {
      key: 'time_stop',
      label: 'Time',
      tooltip:
        'Time stop / max-hold — exit at the first trade this long after entry, cutting positions that neither moon nor crash. Blank/0 = off.',
      group: 'rule_param',
      sortable: true,
      render: (r) =>
        r.p_time_stop_secs ? (
          <span className="font-bold text-info">{formatAge(r.p_time_stop_secs)}</span>
        ) : (
          '-'
        ),
      sortValue: (r) => r.p_time_stop_secs ?? 0,
      searchValue: (r) => String(r.p_time_stop_secs ?? ''),
    },
    {
      key: 'stall',
      label: 'Stall',
      tooltip:
        'Stall / momentum-death — exit once no new higher-high has printed for this long, selling into the flatline. Blank/0 = off.',
      group: 'rule_param',
      sortable: true,
      render: (r) =>
        r.p_stall_secs ? (
          <span className="font-bold text-accent">{formatAge(r.p_stall_secs)}</span>
        ) : (
          '-'
        ),
      sortValue: (r) => r.p_stall_secs ?? 0,
      searchValue: (r) => String(r.p_stall_secs ?? ''),
    },
    {
      key: 'tol',
      label: 'Tolerance',
      tooltip:
        'Match tolerance (%) applied to the numeric entry filters (Init Buy, CU Lim, CU Price, Max SOL, Spendable).',
      group: 'rule_param',
      sortable: true,
      render: (r) => dashPercent(r.tolerance_pct),
      sortValue: (r) => r.tolerance_pct,
      searchValue: (r) => String(r.tolerance_pct),
    },
    {
      key: 'mode',
      label: 'Mode',
      group: 'state',
      sortable: true,
      render: (r) => (
        <Badge
          variant={r.trade_mode === 'real' ? 'danger' : 'info'}
          size="sm"
          pill
          className="uppercase"
        >
          {r.trade_mode === 'real' ? 'Real' : 'Paper'}
        </Badge>
      ),
      sortValue: (r) => r.trade_mode,
      searchValue: (r) => r.trade_mode,
    },
    {
      key: 'status',
      label: 'Status',
      group: 'state',
      sortable: true,
      render: (r) => (
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            onToggleActive(r);
          }}
          className={cn(
            'inline-flex rounded-full border px-2.5 py-0.5 text-[10px] font-bold uppercase tracking-wide transition hover:scale-105',
            r.is_active
              ? 'border-green/40 bg-green/12 text-green'
              : 'border-white/10 bg-white/4 text-text-dim',
          )}
          title="Toggle active/inactive"
        >
          {r.is_active ? '● Active' : '○ Inactive'}
        </button>
      ),
      sortValue: (r) => (r.is_active ? 1 : 0),
      searchValue: (r) => String(r.is_active),
    },
  ];
}
