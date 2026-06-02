import type { ColumnDef } from '../table/types';
import type { RuleRecord } from '../../types';
import { dashF, dashNum, dashPercent } from './utils';
import { cn } from '../../lib/cn';

export function ruleColumns(onToggleActive: (rule: RuleRecord) => void): ColumnDef<RuleRecord>[] {
  return [
    {
      key: 'name',
      label: 'Name',
      sortable: true,
      render: (r) => <span className="font-semibold font-mono">{r.rule_name}</span>,
      sortValue: (r) => r.rule_name,
      searchValue: (r) => r.rule_name,
    },
    {
      key: 'init_buy',
      label: 'Init Buy',
      sortable: true,
      render: (r) => dashF(r.p_initial_buy_sol ?? 0, 15),
      sortValue: (r) => r.p_initial_buy_sol ?? 0,
      searchValue: (r) => String(r.p_initial_buy_sol ?? ''),
    },
    {
      key: 'cu_limit',
      label: 'CU Lim',
      sortable: true,
      render: (r) => dashNum(r.p_cu_limit),
      sortValue: (r) => r.p_cu_limit,
      searchValue: (r) => String(r.p_cu_limit ?? ''),
    },
    {
      key: 'cu_price',
      label: 'CU Price',
      sortable: true,
      render: (r) => dashNum(r.p_cu_price),
      sortValue: (r) => r.p_cu_price,
      searchValue: (r) => String(r.p_cu_price ?? ''),
    },
    {
      key: 'max_sol',
      label: 'Max SOL',
      sortable: true,
      render: (r) => dashF(r.p_max_sol_cost ?? 0, 3),
      sortValue: (r) => r.p_max_sol_cost ?? 0,
      searchValue: (r) => String(r.p_max_sol_cost ?? ''),
    },
    {
      key: 'spendable',
      label: 'Spendable',
      sortable: true,
      render: (r) => dashF(r.p_spendable_sol_in ?? 0, 3),
      sortValue: (r) => r.p_spendable_sol_in ?? 0,
      searchValue: (r) => String(r.p_spendable_sol_in ?? ''),
    },
    {
      key: 'max_hold',
      label: 'Max Hold',
      sortable: true,
      render: (r) => dashNum(r.p_max_holding_tokens),
      sortValue: (r) => r.p_max_holding_tokens,
      searchValue: (r) => String(r.p_max_holding_tokens ?? ''),
    },
    {
      key: 'total_max',
      label: 'Total Max',
      sortable: true,
      render: (r) => dashNum(r.p_total_max_trade_tokens),
      sortValue: (r) => r.p_total_max_trade_tokens,
      searchValue: (r) => String(r.p_total_max_trade_tokens ?? ''),
    },
    {
      key: 'labels',
      label: 'Labels',
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
      key: 'buy_amt',
      label: 'Buy Amt',
      sortable: true,
      render: (r) => dashF(r.buy_amount, 15),
      sortValue: (r) => r.buy_amount,
      searchValue: (r) => String(r.buy_amount),
    },
    {
      key: 'tp',
      label: 'TP',
      sortable: true,
      render: (r) => <span className="font-bold text-green">{dashPercent(r.take_profit)}</span>,
      sortValue: (r) => r.take_profit,
      searchValue: (r) => String(r.take_profit),
    },
    {
      key: 'sl',
      label: 'SL',
      sortable: true,
      render: (r) => <span className="font-bold text-red">{dashPercent(r.stop_loss)}</span>,
      sortValue: (r) => r.stop_loss,
      searchValue: (r) => String(r.stop_loss),
    },
    {
      key: 'tol',
      label: 'Tolerance',
      sortable: true,
      render: (r) => dashPercent(r.tolerance_pct),
      sortValue: (r) => r.tolerance_pct,
      searchValue: (r) => String(r.tolerance_pct),
    },
    {
      key: 'mode',
      label: 'Mode',
      sortable: true,
      render: (r) => (
        <span
          className={cn(
            'inline-block rounded-full px-2 py-0.5 text-[10px] font-bold uppercase',
            r.trade_mode === 'real'
              ? 'border border-red/40 bg-red/12 text-red'
              : 'border border-info/40 bg-info/12 text-info',
          )}
        >
          {r.trade_mode === 'real' ? 'Real' : 'Paper'}
        </span>
      ),
      sortValue: (r) => r.trade_mode,
      searchValue: (r) => r.trade_mode,
    },
    {
      key: 'status',
      label: 'Status',
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
