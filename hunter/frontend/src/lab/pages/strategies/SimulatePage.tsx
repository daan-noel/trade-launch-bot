import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';

import { DataTable } from 'components/table/DataTable';
import type { ColumnDef } from 'components/table/types';
import { Button } from 'components/ui/Button';
import { Badge } from 'components/ui/Badge';
import { dashF, dashPercent } from 'components/strategy/cellFormat';
import { apiErrorMessage } from 'store/baseApi';
import { connectSimulationFinished } from 'services/sse';
import { useGetFingerprintsQuery, useGetStrategyRulesQuery } from 'store/sharedEndpoints';
import { ruleParamsCell, ruleParamsSearchText } from 'components/strategy/RuleParamsSummary';
import {
  fingerprintParamsCell,
  fingerprintParamsSearchText,
} from 'components/strategy/FingerprintParamsSummary';
import { lamportsToSol, type Fingerprint, type StrategyRule } from 'lib/strategy/types';
import type { SimulatedSummary } from 'types';
import {
  useStartEngineSimulationMutation,
  useGetEngineSimSummaryMutation,
} from '@lab/store/labEndpoints';

type RunState = { running: boolean; summary?: SimulatedSummary; error?: string };

const DASH = <span className="text-text-dim/60">—</span>;

/**
 * Full-corpus simulate for saved rules (lab app, FE3.2). Replaces the per-strategy
 * simulate flows with one generic surface: run a saved rule over the whole lake,
 * show its funnel summary as sortable/filterable columns. The dry-run panel
 * (unsaved-draft loop) lives in the rule editor; this page is for persisted rules.
 */
export function SimulatePage() {
  const { data: rules = [], isLoading } = useGetStrategyRulesQuery();
  const { data: fps = [] } = useGetFingerprintsQuery();
  const [start] = useStartEngineSimulationMutation();
  const [fetchSummary] = useGetEngineSimSummaryMutation();
  const [runs, setRuns] = useState<Record<string, RunState>>({});
  const handleRef = useRef<{ close: () => void } | null>(null);

  const fpById = useMemo(() => new Map(fps.map((f) => [f.id, f])), [fps]);

  // One page-level subscription routes each finished run to its rule (run_id ==
  // rule_id for saved rules).
  useEffect(() => {
    handleRef.current = connectSimulationFinished(async (ev) => {
      const id = ev.rule_id;
      if (ev.cancelled) {
        setRuns((r) => ({ ...r, [id]: { running: false } }));
        return;
      }
      try {
        const summary = await fetchSummary(id).unwrap();
        setRuns((r) => ({ ...r, [id]: { running: false, summary } }));
      } catch (e) {
        setRuns((r) => ({ ...r, [id]: { running: false, error: apiErrorMessage(e as never) ?? 'summary failed' } }));
      }
    });
    return () => handleRef.current?.close();
  }, [fetchSummary]);

  const runRule = async (rule: StrategyRule) => {
    setRuns((r) => ({ ...r, [rule.id]: { running: true } }));
    try {
      await start({ rule_id: rule.id }).unwrap();
    } catch (e) {
      setRuns((r) => ({ ...r, [rule.id]: { running: false, error: apiErrorMessage(e as never) ?? 'start failed' } }));
    }
  };

  const columns = useMemo(() => buildColumns(runs, fpById), [runs, fpById]);

  return (
    <div className="flex flex-col gap-3">
      <h1 className="text-lg font-semibold text-text">Simulate</h1>
      <p className="text-[12px] text-text-dim">
        Run a saved rule over the full lake corpus. For unsaved drafts use the dry-run panel
        in the rule editor.
      </p>
      <DataTable
        columns={columns}
        rows={rules}
        rowKey={(r) => r.id}
        loading={isLoading}
        searchable
        tableId="simulate-rules"
        emptyMessage="No rules yet — author one on the Rules page."
        rowActions={(r) => (
          <Button
            variant="primary"
            size="xs"
            disabled={runs[r.id]?.running}
            onClick={() => runRule(r)}
          >
            {runs[r.id]?.running ? 'Running…' : 'Simulate'}
          </Button>
        )}
      />
    </div>
  );
}

function buildColumns(
  runs: Record<string, RunState>,
  fpById: Map<string, Fingerprint>,
): ColumnDef<StrategyRule>[] {
  const runOf = (r: StrategyRule) => runs[r.id];
  const summaryOf = (r: StrategyRule) => runOf(r)?.summary;

  /** One SimulatedSummary numeric field as its own sortable/filterable column. */
  const simMetric = (
    key: string,
    label: string,
    value: (s: SimulatedSummary) => number,
    renderVal: (s: SimulatedSummary) => ReactNode,
    opts?: { tooltip?: string; displayUnits?: (n: number) => number },
  ): ColumnDef<StrategyRule> => {
    const units = opts?.displayUnits ?? ((n: number) => n);
    return {
      key,
      label,
      group: 'sim',
      tooltip: opts?.tooltip,
      sortable: true,
      render: (r) => {
        const run = runOf(r);
        if (!run) return DASH;
        if (run.running) return <span className="text-text-dim">…</span>;
        if (run.error || !run.summary) return DASH;
        return renderVal(run.summary);
      },
      sortValue: (r) => {
        const s = summaryOf(r);
        return s ? value(s) : null;
      },
      filterNumber: (r) => {
        const s = summaryOf(r);
        return s ? units(value(s)) : null;
      },
      filterValue: (r) => {
        const s = summaryOf(r);
        return s ? String(units(value(s))) : '';
      },
      searchValue: (r) => {
        const s = summaryOf(r);
        return s ? String(units(value(s))) : '';
      },
    };
  };

  return [
    {
      key: 'rule_name',
      label: 'Rule',
      render: (r) => <span className="font-medium text-text">{r.rule_name}</span>,
      searchValue: (r) => r.rule_name,
    },
    {
      key: 'status',
      label: 'Status',
      render: (r) => (
        <Badge variant={r.is_active ? 'success' : 'neutral'}>{r.is_active ? 'Active' : 'Idle'}</Badge>
      ),
      searchValue: (r) => (r.is_active ? 'active' : 'idle'),
    },
    {
      key: 'mode',
      label: 'Mode',
      render: (r) => <Badge variant={r.trade_mode === 'real' ? 'warning' : 'info'}>{r.trade_mode}</Badge>,
      searchValue: (r) => r.trade_mode,
    },
    {
      key: 'fingerprint',
      label: 'Fingerprint',
      render: (r) => {
        const fp = fpById.get(r.fingerprint_id);
        return (
          <div className="flex min-w-48 flex-col gap-1">
            <span className="font-mono text-[12px] text-text-dim">
              {fp?.name || r.fingerprint_id.slice(0, 8)}
            </span>
            {fp ? fingerprintParamsCell(fp) : null}
          </div>
        );
      },
      searchValue: (r) => fingerprintParamsSearchText(fpById.get(r.fingerprint_id), r.fingerprint_id),
    },
    {
      key: 'buy',
      label: 'Buy',
      render: (r) => <span className="tabular-nums">{lamportsToSol(r.buy_amount_lamports)}◎</span>,
      searchValue: (r) => String(lamportsToSol(r.buy_amount_lamports)),
      sortValue: (r) => r.buy_amount_lamports,
      sortable: true,
    },
    {
      key: 'caps',
      label: 'Caps',
      render: (r) => (
        <span className="tabular-nums text-text-dim">
          {r.max_concurrent_tokens}/{r.max_total_tokens || '∞'}
        </span>
      ),
      searchValue: (r) => `${r.max_concurrent_tokens}/${r.max_total_tokens}`,
    },
    {
      key: 'params',
      label: 'Params',
      render: (r) => ruleParamsCell(r.params),
      searchValue: (r) => ruleParamsSearchText(r.params),
    },
    {
      key: 'sim_run',
      label: 'Run',
      group: 'sim',
      tooltip: 'Last simulate run status for this rule',
      render: (r) => {
        const run = runOf(r);
        if (!run) return DASH;
        if (run.running) return <span className="text-text-dim">running…</span>;
        if (run.error) return <span className="text-red">{run.error}</span>;
        if (run.summary) return <span className="text-text-dim">done</span>;
        return <span className="text-text-dim/60">cancelled</span>;
      },
      searchValue: (r) => {
        const run = runOf(r);
        if (!run) return '';
        if (run.running) return 'running';
        if (run.error) return run.error;
        if (run.summary) return 'done';
        return 'cancelled';
      },
    },
    simMetric('sim_entered', 'Entered', (s) => s.total_tokens, (s) => (
      <span className="tabular-nums text-text">{s.total_tokens}</span>
    ), { tooltip: 'Tokens that took a position' }),
    simMetric('sim_closed', 'Closed', (s) => s.closed_tokens, (s) => (
      <span className="tabular-nums text-text">{s.closed_tokens}</span>
    ), { tooltip: 'Tokens that closed a position' }),
    simMetric(
      'sim_win_rate',
      'Win %',
      (s) => s.win_rate,
      (s) => <span className="tabular-nums text-text">{dashPercent(s.win_rate * 100)}</span>,
      { tooltip: 'Share of closed tokens with PnL > 0', displayUnits: (n) => n * 100 },
    ),
    simMetric(
      'sim_avg_pnl',
      'Avg PnL',
      (s) => s.avg_pnl_percent,
      (s) => <span className="tabular-nums text-text">{dashPercent(s.avg_pnl_percent)}</span>,
      { tooltip: 'Average PnL % over closed tokens' },
    ),
    simMetric(
      'sim_total_pnl',
      'Total PnL',
      (s) => s.total_pnl_sol,
      (s) => {
        const cls = s.total_pnl_sol >= 0 ? 'text-green' : 'text-red';
        return <span className={`tabular-nums ${cls}`}>{dashF(s.total_pnl_sol, 3)}◎</span>;
      },
      { tooltip: 'Sum of realized PnL in SOL' },
    ),
  ];
}
