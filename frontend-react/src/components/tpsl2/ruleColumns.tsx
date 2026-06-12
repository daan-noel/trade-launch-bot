import type { MouseEvent } from 'react';
import type { ColumnDef } from 'components/table/types';
import type { RuleRecord } from 'types';
import { dashF, dashNum, dashPercent } from './utils';
import { formatAge } from 'utils/format';
import { cn } from 'lib/cn';
import { Badge } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';

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
function RunControls({ rule, c }: { rule: RuleRecord; c: RuleControlHandlers }) {
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

export function ruleColumns(controls: RuleControlHandlers): ColumnDef<RuleRecord>[] {
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
      render: (r) => dashF(r.p_token_initial_buy_sol ?? 0, 15),
      sortValue: (r) => r.p_token_initial_buy_sol ?? 0,
      searchValue: (r) => String(r.p_token_initial_buy_sol ?? ''),
    },
    {
      key: 'cu_limit',
      label: 'CU Lim',
      group: 'token_fingerprint',
      sortable: true,
      render: (r) => dashNum(r.p_token_cu_limit),
      sortValue: (r) => r.p_token_cu_limit,
      searchValue: (r) => String(r.p_token_cu_limit ?? ''),
    },
    {
      key: 'cu_price',
      label: 'CU Price',
      group: 'token_fingerprint',
      sortable: true,
      render: (r) => dashNum(r.p_token_cu_price),
      sortValue: (r) => r.p_token_cu_price,
      searchValue: (r) => String(r.p_token_cu_price ?? ''),
    },
    {
      key: 'max_sol',
      label: 'Max SOL',
      group: 'token_fingerprint',
      sortable: true,
      render: (r) => dashF(r.p_token_max_sol_cost ?? 0, 3),
      sortValue: (r) => r.p_token_max_sol_cost ?? 0,
      searchValue: (r) => String(r.p_token_max_sol_cost ?? ''),
    },
    {
      key: 'spendable',
      label: 'Spendable',
      group: 'token_fingerprint',
      sortable: true,
      render: (r) => dashF(r.p_token_spendable_sol_in ?? 0, 3),
      sortValue: (r) => r.p_token_spendable_sol_in ?? 0,
      searchValue: (r) => String(r.p_token_spendable_sol_in ?? ''),
    },
    {
      key: 'labels',
      label: 'Labels',
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
      tooltip:
        'Match tolerance (%) applied to the numeric token-fingerprint filters (Init Buy, CU Lim, CU Price, Max SOL, Spendable).',
      group: 'token_fingerprint',
      sortable: true,
      render: (r) => dashPercent(r.tolerance_pct),
      sortValue: (r) => r.tolerance_pct,
      searchValue: (r) => String(r.tolerance_pct),
    },
    {
      key: 'max_concurrent_tokens',
      label: 'Max Concurrent Tokens',
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
      tooltip: 'Position size — SOL allocated per buy (paper or real).',
      group: 'sizing',
      sortable: true,
      render: (r) => dashF(r.buy_amount, 15),
      sortValue: (r) => r.buy_amount,
      searchValue: (r) => String(r.buy_amount),
    },
    {
      key: 'min_age',
      label: 'Min Age',
      tooltip:
        'Entry gate (scalp) — only buy once the token is at least this old, skipping the first chaotic seconds. Blank/0 = off.',
      group: 'entry',
      sortable: true,
      render: (r) =>
        r.p_entry_min_age_secs ? (
          <span className="font-mono text-accent">{formatAge(r.p_entry_min_age_secs)}</span>
        ) : (
          '-'
        ),
      sortValue: (r) => r.p_entry_min_age_secs ?? 0,
      searchValue: (r) => String(r.p_entry_min_age_secs ?? ''),
    },
    {
      key: 'min_alive',
      label: 'Min Alive',
      tooltip:
        'Entry gate (scalp) — require at least this much alive (real, un-pulled) SOL in the curve before buying. Blank/0 = off.',
      group: 'entry',
      sortable: true,
      render: (r) => dashF(r.p_entry_min_alive_sol ?? 0, 3),
      sortValue: (r) => r.p_entry_min_alive_sol ?? 0,
      searchValue: (r) => String(r.p_entry_min_alive_sol ?? ''),
    },
    {
      key: 'min_organic',
      label: 'Min Org',
      tooltip:
        'Entry gate (scalp) — require at least this much organic (non-bot) SOL flow before buying. Blank/0 = off.',
      group: 'entry',
      sortable: true,
      render: (r) => dashF(r.p_entry_min_organic_sol ?? 0, 3),
      sortValue: (r) => r.p_entry_min_organic_sol ?? 0,
      searchValue: (r) => String(r.p_entry_min_organic_sol ?? ''),
    },
    {
      key: 'pullback',
      label: 'Pullback',
      tooltip:
        'Entry gate (scalp) — wait for a pullback of at least this % off the peak before buying the continuation. Blank/0 = off.',
      group: 'entry',
      sortable: true,
      render: (r) => dashPercent(r.p_entry_pullback_pct ?? 0),
      sortValue: (r) => r.p_entry_pullback_pct ?? 0,
      searchValue: (r) => String(r.p_entry_pullback_pct ?? ''),
    },
    {
      key: 'higher_low',
      label: 'Higher-Low',
      tooltip:
        'Entry gate (scalp) — require a confirmed higher-low to have held for this long before buying. Blank/0 = off.',
      group: 'entry',
      sortable: true,
      render: (r) =>
        r.p_entry_higher_low_secs ? (
          <span className="font-mono text-accent">{formatAge(r.p_entry_higher_low_secs)}</span>
        ) : (
          '-'
        ),
      sortValue: (r) => r.p_entry_higher_low_secs ?? 0,
      searchValue: (r) => String(r.p_entry_higher_low_secs ?? ''),
    },
    {
      key: 'max_cohort',
      label: 'Max Cohort',
      tooltip:
        'Entry gate (scalp) — skip the buy if the cohort already holds more than this share of supply. Blank/0 = off.',
      group: 'entry',
      sortable: true,
      render: (r) => dashF(r.p_entry_max_cohort_held ?? 0, 3),
      sortValue: (r) => r.p_entry_max_cohort_held ?? 0,
      searchValue: (r) => String(r.p_entry_max_cohort_held ?? ''),
    },
    {
      key: 'min_liq',
      label: 'Min Liq',
      tooltip:
        'Entry gate (scalp) — require at least this much virtual SOL liquidity before buying. Blank/0 = off.',
      group: 'entry',
      sortable: true,
      render: (r) => dashF(r.p_entry_min_liquidity_sol ?? 0, 3),
      sortValue: (r) => r.p_entry_min_liquidity_sol ?? 0,
      searchValue: (r) => String(r.p_entry_min_liquidity_sol ?? ''),
    },
    {
      key: 'min_org_liq',
      label: 'Min Org Liq',
      tooltip:
        'Entry gate (scalp) — require at least this much organic liquidity before buying. Blank/0 = off.',
      group: 'entry',
      sortable: true,
      render: (r) => dashF(r.p_entry_min_organic_liq ?? 0, 3),
      sortValue: (r) => r.p_entry_min_organic_liq ?? 0,
      searchValue: (r) => String(r.p_entry_min_organic_liq ?? ''),
    },
    {
      key: 'tp',
      label: 'TP',
      tooltip: 'Take profit (%) — exit once price rises this far above the entry price.',
      group: 'exit',
      sortable: true,
      render: (r) => <span className="font-bold text-green">{dashPercent(r.p_exit_take_profit)}</span>,
      sortValue: (r) => r.p_exit_take_profit,
      searchValue: (r) => String(r.p_exit_take_profit),
    },
    {
      key: 'sl',
      label: 'SL',
      tooltip: 'Stop loss (%) — exit once price falls this far below the entry price.',
      group: 'exit',
      sortable: true,
      render: (r) => <span className="font-bold text-red">{dashPercent(r.p_exit_stop_loss)}</span>,
      sortValue: (r) => r.p_exit_stop_loss,
      searchValue: (r) => String(r.p_exit_stop_loss),
    },
    {
      key: 'trail',
      label: 'Trail',
      tooltip:
        'Trailing stop (%) — exit when price falls this far below the peak reached since entry, banking a reversal. Blank/0 = off.',
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
      tooltip:
        'Time stop / max-hold — exit at the first trade this long after entry, cutting positions that neither moon nor crash. Blank/0 = off.',
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
      tooltip:
        'Stall / momentum-death — exit once no new higher-high has printed for this long, selling into the flatline. Blank/0 = off.',
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
      tooltip:
        'Liquidity-death exit (%) — bail when virtual SOL reserves crash this far below the peak-since-entry, catching liquidity pulls price stops miss. Blank/0 = off.',
      group: 'exit',
      sortable: true,
      render: (r) => (
        <span className="font-bold text-primary">{dashPercent(r.p_exit_liquidity_drop_pct ?? 0)}</span>
      ),
      sortValue: (r) => r.p_exit_liquidity_drop_pct ?? 0,
      searchValue: (r) => String(r.p_exit_liquidity_drop_pct ?? ''),
    },
    {
      key: 'cohort_exit',
      label: 'Cohort',
      tooltip:
        'Cohort exit ratio — bail when the cohort sheds this fraction of its peak holding, front-running a coordinated dump. Blank/0 = off.',
      group: 'exit',
      sortable: true,
      render: (r) => (
        <span className="font-bold text-red">{dashPercent(r.p_exit_cohort_ratio ?? 0)}</span>
      ),
      sortValue: (r) => r.p_exit_cohort_ratio ?? 0,
      searchValue: (r) => String(r.p_exit_cohort_ratio ?? ''),
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
    {
      key: 'controls',
      label: 'Run',
      group: 'run',
      sortable: false,
      width: '180px',
      render: (r) => <RunControls rule={r} c={controls} />,
      searchValue: () => '',
    },
  ];
}
