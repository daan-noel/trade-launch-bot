import { createContext, useContext, type MouseEvent } from 'react';
import type { ColumnDef } from 'components/table/types';
import type { RuleRecord } from 'types';
import { dashF, dashNum, dashPercent } from 'components/strategy/cellFormat';
import { formatAge } from 'utils/format';
import { paramTip } from 'lib/tpslParamHelp';
import { cn } from 'lib/cn';
import { Badge } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';

/** Run + Analyze handler bags for the per-row controls. The page owns the
 *  lifecycle/result state; it passes both in through `RuleRowProvider` so the
 *  control cells can read them via context instead of forcing `ruleColumns` to
 *  take arguments — every column then declares the same way. */
export interface RuleRowContextValue {
  controls: RuleControlHandlers;
  analysis: RuleAnalysisHandlers;
}

const RuleRowContext = createContext<RuleRowContextValue | null>(null);

/** Wrap the rules `DataTable` in this so the Run/Analyze cells can reach the
 *  page's handlers. */
export const RuleRowProvider = RuleRowContext.Provider;

function useRuleRow(): RuleRowContextValue {
  const ctx = useContext(RuleRowContext);
  if (!ctx) throw new Error('Rule control cell rendered outside <RuleRowProvider>');
  return ctx;
}

/** Sign-coloured class for a realized-PnL figure: green gain, red loss, dim flat. */
function pnlClass(v: number): string {
  if (v > 0) return 'text-green';
  if (v < 0) return 'text-red';
  return 'text-text-dim';
}

/** Read-only lifecycle badge. The clickable activate/pause/stop controls now
 *  live in the row actions; this column just *names* the state so it reads at a
 *  glance — the amber `Draining` answers "why is an inactive rule still trading?"
 *  (its open positions are still exiting). */
function LifecycleBadge({ rule }: { rule: RuleRecord }) {
  const style: Record<string, { cls: string; label: string }> = {
    Active: { cls: 'border-green/40 bg-green/12 text-green', label: '● Active' },
    Draining: {
      cls: 'border-warning/40 bg-warning/12 text-warning',
      label: `◐ Draining · ${rule.open_positions}`,
    },
    Finished: { cls: 'border-info/40 bg-info/12 text-info', label: '✓ Finished' },
    Idle: { cls: 'border-white/10 bg-white/4 text-text-dim', label: '○ Idle' },
  };
  const s = style[rule.lifecycle] ?? style.Idle;
  return (
    <span
      className={cn(
        'inline-flex rounded-full border px-2.5 py-0.5 text-[10px] font-bold uppercase tracking-wide',
        s.cls,
      )}
      title={
        rule.lifecycle === 'Draining'
          ? `Inactive — ${rule.open_positions} open position(s) still exiting`
          : rule.lifecycle
      }
    >
      {s.label}
    </span>
  );
}

/** Callbacks + busy state the Run-control column needs. Supplied by the page so
 *  the column can live in `ruleColumns` while the lifecycle handlers stay on the
 *  page. */
export interface RuleControlHandlers {
  /** Id of the rule currently mid-transition (its buttons are disabled). */
  busyId: string | null;
  onPause: (rule: RuleRecord) => void;
  onResume: (rule: RuleRecord) => void;
  onStop: (rule: RuleRecord) => void;
  onActivate: (rule: RuleRecord) => void;
}

/** State-aware run controls, rendered in their own column:
 *   - Active        → Pause · Stop
 *   - Draining (>0) → Resume · Stop
 *   - Idle/Finished → Activate
 *  Activate is visually distinct (green + outline ▷) from the teal filled ▶ Resume so
 *  "start fresh" never reads the same as "continue". Clicks stop propagation so
 *  they don't also select the row. */
function RunControls({ rule }: { rule: RuleRecord }) {
  const { controls: c } = useRuleRow();
  const busy = c.busyId === rule.id;
  const stop = (fn: () => void) => (e: MouseEvent) => {
    e.stopPropagation();
    fn();
  };

  if (rule.is_active) {
    return (
      <div className="flex items-center gap-1 justify-center">
        <Button
          variant="ghost"
          size="xs"
          disabled={busy}
          onClick={stop(() => c.onPause(rule))}
          title="Pause — stop new entries; open positions keep draining"
        >
          ⏸ Pause
        </Button>
        <Button
          variant="danger"
          size="xs"
          disabled={busy}
          onClick={stop(() => c.onStop(rule))}
          title="Stop & close all open positions now"
        >
          ■ Stop
        </Button>
      </div>
    );
  }

  if (rule.open_positions > 0) {
    return (
      <div className="flex items-center gap-1 justify-center">
        <Button
          variant="primary"
          size="xs"
          disabled={busy}
          onClick={stop(() => c.onResume(rule))}
          title="Resume — turn entries back on and keep the open positions (continues the run)"
        >
          ▶ Resume
        </Button>
        <Button
          variant="danger"
          size="xs"
          disabled={busy}
          onClick={stop(() => c.onStop(rule))}
          title="Force-close the remaining open positions now"
        >
          ■ Stop
        </Button>
      </div>
    );
  }

  return (
    <Button
      variant="ghost"
      size="xs"
      disabled={busy}
      onClick={stop(() => c.onActivate(rule))}
      className="border-green/50 bg-green/10 font-semibold text-green hover:border-green/70 hover:bg-green/20 hover:text-green"
      title="Activate — start taking entries"
    >
      ▷ Activate
    </Button>
  );
}

/** Callbacks + per-row state the Analyze column needs. Supplied by the page so
 *  the column lives here while the result-view handlers stay on the page. The
 *  `*ActiveId` fields highlight the button whose result panel is currently open;
 *  the `*Loading` flags disable a tool while its fetch is in flight. */
export interface RuleAnalysisHandlers {
  simLoading: boolean;
  matchedLoading: boolean;
  paperLoading: boolean;
  matchedActiveId: string | null;
  paperActiveId: string | null;
  onSimulate: (rule: RuleRecord) => void;
  onMatched: (rule: RuleRecord) => void;
  onPaperResult: (rule: RuleRecord) => void;
}

/** Read-only analysis tools, in their own column so they read as a group
 *  distinct from the rule-management actions (Edit/Del). Icon-only — each
 *  button's tooltip names the action. Clicks stop propagation so they inspect
 *  the rule without also selecting the row. */
function AnalysisControls({ rule }: { rule: RuleRecord }) {
  const { analysis: a } = useRuleRow();
  const stop = (fn: () => void) => (e: MouseEvent) => {
    e.stopPropagation();
    fn();
  };
  const matchedActive = a.matchedActiveId === rule.id;
  const paperActive = a.paperActiveId === rule.id;
  return (
    <div className="flex items-center gap-1 justify-center">
      <Button
        variant="ghost"
        size="xs"
        disabled={a.simLoading}
        onClick={stop(() => a.onSimulate(rule))}
        className="text-primary"
        title="Simulate — backtest this rule over historical tokens"
      >
        🧪
      </Button>
      <Button
        variant="ghost"
        size="xs"
        disabled={a.matchedLoading}
        onClick={stop(() => a.onMatched(rule))}
        className={cn(matchedActive && 'border-[#9370db]/45 bg-[#9370db]/8 text-[#9370db]')}
        title="Matched tokens — tokens in the DB that pass this rule's entry filter"
      >
        🎯
      </Button>
      {rule.trade_mode === 'paper' && (
        <Button
          variant="ghost"
          size="xs"
          disabled={a.paperLoading}
          onClick={stop(() => a.onPaperResult(rule))}
          className={cn('text-info', paperActive && 'border-info/45 bg-info/8')}
          title="Paper test result — positions from the latest paper run"
        >
          📄
        </Button>
      )}
    </div>
  );
}

export const ruleColumns: ColumnDef<RuleRecord>[] = [
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
      tooltip: paramTip('initialBuy'),
      group: 'token_fingerprint',
      sortable: true,
      render: (r) => dashF(r.p_token_initial_buy_sol ?? 0, 15),
      sortValue: (r) => r.p_token_initial_buy_sol ?? 0,
      searchValue: (r) => String(r.p_token_initial_buy_sol ?? ''),
    },
    {
      key: 'cu_limit',
      label: 'CU Lim',
      tooltip: paramTip('cuLimit'),
      group: 'token_fingerprint',
      sortable: true,
      render: (r) => dashNum(r.p_token_cu_limit),
      sortValue: (r) => r.p_token_cu_limit,
      searchValue: (r) => String(r.p_token_cu_limit ?? ''),
    },
    {
      key: 'cu_price',
      label: 'CU Price',
      tooltip: paramTip('cuPrice'),
      group: 'token_fingerprint',
      sortable: true,
      render: (r) => dashNum(r.p_token_cu_price),
      sortValue: (r) => r.p_token_cu_price,
      searchValue: (r) => String(r.p_token_cu_price ?? ''),
    },
    {
      key: 'max_sol',
      label: 'Max SOL',
      tooltip: paramTip('maxSolCost'),
      group: 'token_fingerprint',
      sortable: true,
      render: (r) => dashF(r.p_token_max_sol_cost ?? 0, 3),
      sortValue: (r) => r.p_token_max_sol_cost ?? 0,
      searchValue: (r) => String(r.p_token_max_sol_cost ?? ''),
    },
    {
      key: 'spendable',
      label: 'Spendable',
      tooltip: paramTip('spendableSolIn'),
      group: 'token_fingerprint',
      sortable: true,
      render: (r) => dashF(r.p_token_spendable_sol_in ?? 0, 3),
      sortValue: (r) => r.p_token_spendable_sol_in ?? 0,
      searchValue: (r) => String(r.p_token_spendable_sol_in ?? ''),
    },
    {
      key: 'labels',
      label: 'Labels',
      tooltip: paramTip('ixLabels'),
      group: 'token_fingerprint',
      sortable: true,
      render: (r) => {
        const arr = Array.isArray(r.p_token_ix_labels) ? r.p_token_ix_labels : [];
        const tooltip = arr.map(String).join('\n');
        return (
          <span title={tooltip} className="font-mono">
            {arr.length > 0 ? arr.length : '-'}
          </span>
        );
      },
      sortValue: (r) => (Array.isArray(r.p_token_ix_labels) ? r.p_token_ix_labels.length : 0),
      searchValue: (r) =>
        Array.isArray(r.p_token_ix_labels) ? r.p_token_ix_labels.map(String).join(' ') : '',
    },
    {
      key: 'tol',
      label: 'Tolerance',
      tooltip: paramTip('tolerance'),
      group: 'token_fingerprint',
      sortable: true,
      render: (r) => dashPercent(r.tolerance_pct),
      sortValue: (r) => r.tolerance_pct,
      searchValue: (r) => String(r.tolerance_pct),
    },
    {
      key: 'max_concurrent_tokens',
      label: 'Max Concurrent Tokens',
      tooltip: paramTip('maxConcurrentTokens'),
      group: 'sizing',
      width: '120px',
      sortable: true,
      render: (r) => dashNum(r.p_max_concurrent_tokens),
      sortValue: (r) => r.p_max_concurrent_tokens,
      searchValue: (r) => String(r.p_max_concurrent_tokens ?? ''),
    },
    {
      key: 'max_total_tokens',
      label: 'Max Total Tokens',
      tooltip: paramTip('maxTotalTokens'),
      group: 'sizing',
      width: '120px',
      sortable: true,
      render: (r) => dashNum(r.p_max_total_tokens),
      sortValue: (r) => r.p_max_total_tokens,
      searchValue: (r) => String(r.p_max_total_tokens ?? ''),
    },
    {
      key: 'buy_amt',
      label: 'Buy Amt',
      tooltip: paramTip('buyAmount'),
      group: 'sizing',
      sortable: true,
      render: (r) => dashF(r.buy_amount_sol, 15),
      sortValue: (r) => r.buy_amount_sol,
      searchValue: (r) => String(r.buy_amount_sol),
    },
    {
      key: 'tp',
      label: 'TP',
      tooltip: paramTip('takeProfit'),
      group: 'exit',
      sortable: true,
      render: (r) => <span className="font-bold text-green">{dashPercent(r.p_exit_take_profit)}</span>,
      sortValue: (r) => r.p_exit_take_profit,
      searchValue: (r) => String(r.p_exit_take_profit),
    },
    {
      key: 'sl',
      label: 'SL',
      tooltip: paramTip('stopLoss'),
      group: 'exit',
      sortable: true,
      render: (r) => <span className="font-bold text-red">{dashPercent(r.p_exit_stop_loss)}</span>,
      sortValue: (r) => r.p_exit_stop_loss,
      searchValue: (r) => String(r.p_exit_stop_loss),
    },
    {
      key: 'trail',
      label: 'Trail',
      tooltip: paramTip('trailingStopPct'),
      group: 'exit',
      sortable: true,
      render: (r) => (
        <span className="font-bold text-warning">{dashPercent(r.p_exit_trailing_stop_pct ?? 0)}</span>
      ),
      sortValue: (r) => r.p_exit_trailing_stop_pct ?? 0,
      searchValue: (r) => String(r.p_exit_trailing_stop_pct ?? ''),
    },
    {
      key: 'time_stop',
      label: 'Time',
      tooltip: paramTip('timeStopSecs'),
      group: 'exit',
      sortable: true,
      render: (r) =>
        r.p_exit_time_stop_secs ? (
          <span className="font-bold text-info">{formatAge(r.p_exit_time_stop_secs)}</span>
        ) : (
          '-'
        ),
      sortValue: (r) => r.p_exit_time_stop_secs ?? 0,
      searchValue: (r) => String(r.p_exit_time_stop_secs ?? ''),
    },
    {
      key: 'stall',
      label: 'Stall',
      tooltip: paramTip('stallSecs'),
      group: 'exit',
      sortable: true,
      render: (r) =>
        r.p_exit_stall_secs ? (
          <span className="font-bold text-accent">{formatAge(r.p_exit_stall_secs)}</span>
        ) : (
          '-'
        ),
      sortValue: (r) => r.p_exit_stall_secs ?? 0,
      searchValue: (r) => String(r.p_exit_stall_secs ?? ''),
    },
    {
      key: 'liq',
      label: 'Liq',
      tooltip: paramTip('liquidityDropPct'),
      group: 'exit',
      sortable: true,
      render: (r) => (
        <span className="font-bold text-primary">{dashPercent(r.p_exit_liquidity_drop_pct ?? 0)}</span>
      ),
      sortValue: (r) => r.p_exit_liquidity_drop_pct ?? 0,
      searchValue: (r) => String(r.p_exit_liquidity_drop_pct ?? ''),
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
      render: (r) => <LifecycleBadge rule={r} />,
      sortValue: (r) => r.lifecycle,
      searchValue: (r) => r.lifecycle,
    },
    // Realized-performance group — all-time for real rules, current-run for
    // paper. Backed by the runtime cache (no per-row fetch); see RuleRecord.
    {
      key: 'total_positions',
      label: 'Total',
      tooltip: 'Entered positions — all-time for real rules, current run for paper.',
      group: 'performance',
      sortable: true,
      render: (r) => <span className="font-mono">{dashNum(r.total_positions)}</span>,
      sortValue: (r) => r.total_positions,
      searchValue: (r) => String(r.total_positions),
    },
    {
      key: 'open_positions',
      label: 'Hold',
      tooltip: 'Positions currently held open.',
      group: 'performance',
      sortable: true,
      render: (r) => <span className="font-mono text-info">{dashNum(r.open_positions)}</span>,
      sortValue: (r) => r.open_positions,
      searchValue: (r) => String(r.open_positions),
    },
    {
      key: 'pending_positions',
      label: 'Pending',
      tooltip: 'Armed or buy-in-flight positions not yet filled (Arming / BuySubmitted).',
      group: 'performance',
      sortable: true,
      render: (r) => <span className="font-mono text-warning">{dashNum(r.pending_positions)}</span>,
      sortValue: (r) => r.pending_positions,
      searchValue: (r) => String(r.pending_positions),
    },
    {
      key: 'win_count',
      label: 'Win',
      tooltip: 'Closed positions that sold above entry.',
      group: 'performance',
      sortable: true,
      render: (r) => <span className="font-mono text-green">{dashNum(r.win_count)}</span>,
      sortValue: (r) => r.win_count,
      searchValue: (r) => String(r.win_count),
    },
    {
      key: 'loss_count',
      label: 'Loss',
      tooltip: 'Closed positions at or below entry (failed exits count as losses).',
      group: 'performance',
      sortable: true,
      render: (r) => <span className="font-mono text-red">{dashNum(r.loss_count)}</span>,
      sortValue: (r) => r.loss_count,
      searchValue: (r) => String(r.loss_count),
    },
    {
      key: 'win_rate',
      label: 'Win %',
      tooltip: 'Wins ÷ closed positions.',
      group: 'performance',
      sortable: true,
      render: (r) => <span className="font-mono font-bold">{dashPercent(r.win_rate)}</span>,
      sortValue: (r) => r.win_rate,
      searchValue: (r) => String(r.win_rate),
    },
    {
      key: 'avg_pnl_pct',
      label: 'Return %',
      tooltip:
        'Capital-weighted realized return: total SOL PnL ÷ total SOL deployed across closed positions. Sign always matches PnL ◎.',
      group: 'performance',
      sortable: true,
      render: (r) => (
        <span className={cn('font-mono font-bold', pnlClass(r.avg_pnl_pct))}>
          {dashPercent(r.avg_pnl_pct)}
        </span>
      ),
      sortValue: (r) => r.avg_pnl_pct,
      searchValue: (r) => String(r.avg_pnl_pct),
    },
    {
      key: 'total_pnl_sol',
      label: 'PnL ◎',
      tooltip: 'Total realized SOL PnL across closed positions.',
      group: 'performance',
      sortable: true,
      render: (r) => (
        <span className={cn('font-mono font-bold', pnlClass(r.total_pnl_sol))}>
          {dashF(r.total_pnl_sol, 4)}
        </span>
      ),
      sortValue: (r) => r.total_pnl_sol,
      searchValue: (r) => String(r.total_pnl_sol),
    },
    {
      key: 'controls',
      label: 'Run',
      group: 'run',
      sortable: false,
      width: '180px',
      render: (r) => <RunControls rule={r} />,
      searchValue: () => '',
    },
    {
      key: 'analyze',
      label: 'Analyze',
      group: 'analyze',
      sortable: false,
      width: '110px',
      render: (r) => <AnalysisControls rule={r} />,
      searchValue: () => '',
    }
];
