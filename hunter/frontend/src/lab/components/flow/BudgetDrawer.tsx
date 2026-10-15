import { useCallback, useEffect, useMemo, useState, type CSSProperties } from 'react';
import { createPortal } from 'react-dom';

import { DataTable } from 'components/table/DataTable';
import type { ColumnDef } from 'components/table/types';
import { LENS_COLORS } from 'components/token-price-chart/constants';
import { LensButton } from 'components/tokens/LensControls';
import { Checkbox } from 'components/ui/Checkbox';
import { IxLabelsDisplay } from 'components/ui/IxLabelsDisplay';
import { formatFeeVariant } from 'lib/feeVariants';
import { useGetFlowBudgetsQuery } from '@lab/store/labEndpoints';
import type { FlowDiscoveryFeeVariant, FlowDiscoveryStructure } from 'types';

import type { BudgetState, RankedLens } from './StructureTable';

/** Column visibility + sort prefs persist under this id. */
const TABLE_ID = 'flow-budgets';

const budgetKey = (v: FlowDiscoveryFeeVariant) => `${v.cu_limit}|${v.cu_price}|${v.tip_lamports}`;

const STATE_FILTER: Record<BudgetState, string> = { on: 'pinned', broad: 'any budget', off: 'open', none: 'unpinnable' };
const STATE_SORT: Record<BudgetState, number> = { on: 3, broad: 2, off: 1, none: 0 };

function fmtNum(n: number | null | undefined): string {
  return n == null ? '—' : n.toLocaleString();
}

function fmtSol(n: number | null | undefined): string {
  return n == null || !Number.isFinite(n) ? '—' : n.toFixed(2);
}

/** The run a drawer reads its budgets from. */
export interface BudgetDrawerSource {
  runId: string;
  groupKey: Record<string, unknown>;
  /** The ranked rows are cores, so `s.ix_labels` are core labels. */
  core: boolean;
}

/**
 * Every budget of one ranked row as a table of its own: sortable, searchable,
 * filterable, each line with the same lens and stage check the Budget cell's
 * head carries. A side panel with no backdrop, so the ranked table and the
 * preview chart stay live while a budget's highlight washes it.
 *
 * Opens on the row's clipped `fee_variants` and swaps to the full list the
 * budgets endpoint returns, so the head budgets show at once.
 */
export function BudgetDrawer({
  s,
  source,
  budgetStateOf,
  onToggleBudget,
  lens,
  tag,
  onClose,
}: {
  s: FlowDiscoveryStructure;
  source: BudgetDrawerSource | null;
  budgetStateOf: (s: FlowDiscoveryStructure, v: FlowDiscoveryFeeVariant) => BudgetState;
  onToggleBudget: (labels: string[], v: FlowDiscoveryFeeVariant) => void;
  lens: RankedLens | null;
  tag: string;
  onClose: () => void;
}) {
  const clipped = s.fee_variants ?? [];
  const total = s.fee_variant_count ?? clipped.length;
  const needFull = source != null && total > clipped.length;
  const fullQ = useGetFlowBudgetsQuery(
    {
      run_id: source?.runId ?? '',
      group_key: source?.groupKey ?? {},
      ix_labels: s.ix_labels,
      core: source?.core ?? false,
    },
    { skip: !needFull },
  );
  const full = needFull ? fullQ.currentData : undefined;
  const variants = full?.fee_variants ?? clipped;
  const unknown = full?.fee_unknown_trades ?? s.fee_unknown_trades ?? 0;
  const known = useMemo(() => variants.reduce((sum, v) => sum + v.n_trades, 0), [variants]);
  const fullError = needFull && fullQ.error
    ? ((fullQ.error as { data?: { error?: string } }).data?.error ?? 'Could not load every budget.')
    : null;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !e.defaultPrevented) onClose();
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [onClose]);

  const [filtered, setFiltered] = useState<FlowDiscoveryFeeVariant[]>([]);
  const filteredOff = filtered.filter((v) => budgetStateOf(s, v) === 'off');
  const filteredOn = filtered.filter((v) => budgetStateOf(s, v) === 'on');
  const stageAll = (vs: FlowDiscoveryFeeVariant[]) => vs.forEach((v) => onToggleBudget(s.ix_labels, v));

  const columns = useMemo<ColumnDef<FlowDiscoveryFeeVariant>[]>(() => {
    const share = (v: FlowDiscoveryFeeVariant) => (known > 0 ? (v.n_trades / known) * 100 : 0);
    const cols: ColumnDef<FlowDiscoveryFeeVariant>[] = [];
    if (lens) {
      cols.push({
        key: 'lens',
        label: '◐',
        tooltip: 'Highlight every candle and trade of this shape on this budget',
        render: (v) => {
          const color = lens.budgetColorOf(s, v);
          return (
            <LensButton
              armed={color != null}
              color={color ?? LENS_COLORS.structure[0]}
              title={
                color != null
                  ? 'Stop highlighting this budget'
                  : 'Highlight every candle and trade of this shape on this budget (the highlight pin fields when any are on, else all three)'
              }
              onClick={() => lens.toggleBudget(s, v)}
            />
          );
        },
        sortValue: (v) => (lens.budgetColorOf(s, v) != null ? 1 : 0),
        searchValue: () => '',
        width: '28px',
      });
    }
    cols.push(
      {
        key: 'staged',
        label: tag,
        tooltip: `Stage this shape into ${tag} pinned to one budget`,
        render: (v) => {
          const state = budgetStateOf(s, v);
          return (
            <Checkbox
              checked={state === 'on' || state === 'broad'}
              disabled={state === 'none'}
              onChange={() => onToggleBudget(s.ix_labels, v)}
              title={
                state === 'none'
                  ? 'The fee-pin fields on pick nothing this budget carries - turn on a field it has'
                  : state === 'broad'
                    ? `In ${tag} through the whole-shape row (any budget). Click to narrow the shape to this budget only.`
                    : state === 'on'
                      ? `In ${tag} pinned to this budget. Click to remove it; other budgets stay.`
                      : `Add this shape to ${tag} pinned to this budget only.`
              }
            />
          );
        },
        sortValue: (v) => STATE_SORT[budgetStateOf(s, v)],
        searchValue: () => '',
        filterOptions: (Object.keys(STATE_FILTER) as BudgetState[]).map((k) => ({ value: k, label: STATE_FILTER[k] })),
        filterOptionValue: (v) => budgetStateOf(s, v),
        width: '44px',
      },
      {
        key: 'budget',
        label: 'Budget',
        tooltip: 'The budget as one line: CU limit @ CU price (micro-lamports per CU) + tip (lamports)',
        render: (v) => <span className="whitespace-nowrap text-text">{formatFeeVariant(v)}</span>,
        searchValue: (v) => formatFeeVariant(v),
        defaultVisible: false,
      },
      {
        key: 'cu_limit',
        label: 'CU limit',
        render: (v) => fmtNum(v.cu_limit),
        sortValue: (v) => v.cu_limit,
        searchValue: (v) => String(v.cu_limit ?? ''),
        filterNumber: (v) => v.cu_limit,
      },
      {
        key: 'cu_price',
        label: 'CU price',
        tooltip: 'Micro-lamports per compute unit',
        render: (v) => fmtNum(v.cu_price),
        sortValue: (v) => v.cu_price,
        searchValue: (v) => String(v.cu_price ?? ''),
        filterNumber: (v) => v.cu_price,
      },
      {
        key: 'tip_lamports',
        label: 'Tip',
        tooltip: 'Tip, lamports',
        render: (v) => fmtNum(v.tip_lamports),
        sortValue: (v) => v.tip_lamports,
        searchValue: (v) => String(v.tip_lamports ?? ''),
        filterNumber: (v) => v.tip_lamports,
      },
      {
        key: 'priority_lamports',
        label: 'Priority',
        tooltip: 'Compute rail plus tip, lamports - the money the budget pays',
        render: (v) => fmtNum(v.priority_lamports),
        sortValue: (v) => v.priority_lamports,
        searchValue: () => '',
        filterNumber: (v) => v.priority_lamports,
      },
      {
        key: 'n_trades',
        label: 'Trades',
        render: (v) => v.n_trades.toLocaleString(),
        sortValue: (v) => v.n_trades,
        searchValue: () => '',
        filterNumber: (v) => v.n_trades,
      },
      {
        key: 'share',
        label: 'Share %',
        tooltip: "This budget's share of the row's trades that carry a fee reading",
        render: (v) => share(v).toFixed(1),
        sortValue: share,
        searchValue: () => '',
        filterNumber: share,
      },
      {
        key: 'gross_sol',
        label: 'Gross ◎',
        render: (v) => fmtSol(v.gross_sol),
        sortValue: (v) => v.gross_sol,
        searchValue: () => '',
        filterNumber: (v) => v.gross_sol,
      },
      {
        key: 'buy_sol',
        label: 'Buy ◎',
        render: (v) => fmtSol(v.buy_sol),
        sortValue: (v) => v.buy_sol ?? null,
        searchValue: () => '',
        filterNumber: (v) => v.buy_sol ?? null,
        defaultVisible: false,
      },
      {
        key: 'sell_sol',
        label: 'Sell ◎',
        render: (v) => fmtSol(v.sell_sol),
        sortValue: (v) => v.sell_sol ?? null,
        searchValue: () => '',
        filterNumber: (v) => v.sell_sol ?? null,
        defaultVisible: false,
      },
    );
    return cols;
  }, [s, known, lens, tag, budgetStateOf, onToggleBudget]);

  // A lensed budget washes in its lens color, a staged one in the accent: the
  // ranked table's row convention.
  const rowClassName = useCallback(
    (v: FlowDiscoveryFeeVariant) => {
      if (lens?.budgetColorOf(s, v) != null) return 'bg-(--row-lens)/16 hover:bg-(--row-lens)/24';
      return budgetStateOf(s, v) === 'on' ? 'bg-accent/8' : undefined;
    },
    [s, lens, budgetStateOf],
  );
  const rowStyle = useMemo(() => {
    if (!lens) return undefined;
    return (v: FlowDiscoveryFeeVariant): CSSProperties | undefined => {
      const color = lens.budgetColorOf(s, v);
      return color != null ? ({ '--row-lens': color } as CSSProperties) : undefined;
    };
  }, [s, lens]);

  return createPortal(
    <aside
      role="complementary"
      aria-label="Budgets of the ix structure"
      className="fixed inset-y-0 right-0 z-[150] flex w-[min(640px,96vw)] flex-col border-l border-white/8 bg-bg-panel shadow-[-12px_0_48px_rgba(0,0,0,0.55)]"
    >
      <div className="flex items-start justify-between gap-3 border-b border-white/6 px-4 py-3">
        <div className="min-w-0 flex-1">
          <h2 className="text-[13px] font-bold text-text">
            Budgets of {source?.core ? 'core' : 'shape'}
          </h2>
          <div className="mt-1 text-[11px]">
            <IxLabelsDisplay labels={s.ix_labels} />
          </div>
          <div className="mt-1.5 text-[11px] text-text-dim">
            {total.toLocaleString()} budget{total === 1 ? '' : 's'} · {known.toLocaleString()} trades with a fee
            reading
            {unknown > 0 && <> · {unknown.toLocaleString()} without</>}
            {needFull && fullQ.isFetching && <> · loading all {total.toLocaleString()}…</>}
          </div>
          {fullError && <div className="mt-1 text-[11px] text-red">{fullError} Showing the top {clipped.length}.</div>}
        </div>
        <button
          type="button"
          onClick={onClose}
          title="Close (Esc)"
          className="rounded px-2 py-1 text-2xl leading-none text-text-dim hover:bg-white/6 hover:text-text"
        >
          ×
        </button>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-3">
        <DataTable
          columns={columns}
          rows={variants}
          rowKey={budgetKey}
          rowClassName={rowClassName}
          rowStyle={rowStyle}
          tableId={TABLE_ID}
          searchable
          searchPlaceholder="Search budgets…"
          colFilters
          colToggle
          defaultSort={{ col: 'n_trades', dir: 'desc' }}
          selectable={false}
          // Unpaginated: the stage buttons act on every row the filters keep.
          paginate={false}
          emptyMessage="No budget matches these filters."
          onFilteredRowsChange={setFiltered}
          toolbarTrailing={
            <div className="flex items-center gap-1.5">
              <button
                type="button"
                disabled={filteredOff.length === 0}
                onClick={() => stageAll(filteredOff)}
                className="rounded border border-accent/40 px-2 py-1 text-[11px] font-semibold text-accent transition hover:bg-accent/10 disabled:cursor-not-allowed disabled:opacity-40"
                title={`Stage the shape into ${tag} pinned to each open budget the filters show`}
              >
                Stage filtered{filteredOff.length > 0 ? ` · ${filteredOff.length}` : ''}
              </button>
              <button
                type="button"
                disabled={filteredOn.length === 0}
                onClick={() => stageAll(filteredOn)}
                className="rounded border border-border px-2 py-1 text-[11px] font-semibold text-text-dim transition hover:bg-white/5 hover:text-text disabled:cursor-not-allowed disabled:opacity-40"
                title={`Remove each pinned budget the filters show from ${tag}. A whole-shape row stays.`}
              >
                Unstage filtered{filteredOn.length > 0 ? ` · ${filteredOn.length}` : ''}
              </button>
            </div>
          }
        />
      </div>
    </aside>,
    document.body,
  );
}
