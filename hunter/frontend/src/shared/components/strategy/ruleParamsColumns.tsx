// Trade columns for Rules + Simulate. One column per field. A header sub-row
// only when the field is two numbers (Caps, Again). Take profit and stop loss
// are badges on the Chain cell.

import type { ReactNode } from 'react';

import type { ColumnDef } from 'components/table/types';
import { MultiSortHeader } from 'components/table/MultiSortHeader';
import { lamportsToSol } from 'lib/strategy/types';
import { formatDecimalTrim } from 'utils/format';

import { buildCapsColumns, type CapsRuleRow } from './capsRuleColumns';
import {
  ruleChainCell,
  ruleParamsSearchText,
  ruleTradeFacts,
} from './RuleParamsSummary';

export type RuleTradeRow = CapsRuleRow & {
  buy_amount_lamports: number;
  params: unknown;
};

function dash(): ReactNode {
  return <span className="text-text-dim">—</span>;
}

function buyLabel(r: RuleTradeRow): string {
  const sol = lamportsToSol(r.buy_amount_lamports);
  const pool = ruleTradeFacts(r.params).size_pct_of_pool;
  const amount = `${sol ?? '—'}◎`;
  return pool == null ? amount : `${amount} · ${formatDecimalTrim(pool, 2)}%`;
}

function againLabel(r: RuleTradeRow): string | null {
  const f = ruleTradeFacts(r.params);
  if (f.cooldown_sec == null || f.max_per_coin == null) return null;
  return `${formatDecimalTrim(f.cooldown_sec, 1)}s ×${f.max_per_coin}`;
}

const AGAIN_SORT = [
  {
    key: 'again_cool',
    label: 'cool',
    title: 'Cooldown before this rule may buy the same coin again',
    sortValue: (r: RuleTradeRow) => ruleTradeFacts(r.params).cooldown_sec,
  },
  {
    key: 'again_max',
    label: 'max',
    title: 'Most buys of one coin, including the first',
    sortValue: (r: RuleTradeRow) => ruleTradeFacts(r.params).max_per_coin,
  },
] as const;

/**
 * Buy, Caps, Again, Chain — the same columns on Rules and Simulate.
 */
export function buildRuleTradeColumns<R extends RuleTradeRow>(): ColumnDef<R>[] {
  const buy: ColumnDef<R> = {
    key: 'buy',
    label: 'Buy',
    group: 'trade',
    render: (r) => <span className="tabular-nums">{buyLabel(r)}</span>,
    searchValue: (r) => buyLabel(r),
    sortValue: (r) => r.buy_amount_lamports,
    filterNumber: (r) => lamportsToSol(r.buy_amount_lamports),
    sortable: true,
  };

  const again: ColumnDef<R> = {
    key: 'again',
    label: 'Again',
    group: 'trade',
    render: (r) => {
      const text = againLabel(r);
      return text == null ? dash() : <span className="tabular-nums">{text}</span>;
    },
    searchValue: (r) => againLabel(r) ?? '',
    filterNumber: (r) => ruleTradeFacts(r.params).cooldown_sec,
    renderHeader: (ctx) => <MultiSortHeader title="Again" axes={AGAIN_SORT} ctx={ctx} />,
  };

  const againSort: ColumnDef<R>[] = AGAIN_SORT.map((axis) => ({
    key: axis.key,
    label: axis.label,
    group: 'trade',
    sortOnly: true,
    defaultVisible: false,
    sortable: true,
    render: () => null,
    searchValue: () => '',
    sortValue: (r) => axis.sortValue(r),
  }));

  const chain: ColumnDef<R> = {
    key: 'params',
    label: 'Chain',
    group: 'trade',
    render: (r) => ruleChainCell(r.params),
    searchValue: (r) => ruleParamsSearchText(r.params),
  };

  return [
    buy,
    ...buildCapsColumns<R>(),
    again,
    ...againSort,
    chain,
  ];
}
