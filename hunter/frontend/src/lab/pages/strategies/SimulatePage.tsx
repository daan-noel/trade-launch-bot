import { useEffect, useMemo, useRef, useState } from 'react';

import { DataTable } from 'components/table/DataTable';
import type { ColumnDef } from 'components/table/types';
import { Button } from 'components/ui/Button';
import { Badge } from 'components/ui/Badge';
import { apiErrorMessage } from 'store/baseApi';
import { connectSimulationFinished } from 'services/sse';
import { useGetFingerprintsQuery, useGetStrategyRulesQuery } from 'store/sharedEndpoints';
import { ruleParamsCell, ruleParamsSearchText } from 'components/strategy/RuleParamsSummary';
import { lamportsToSol, type StrategyRule } from 'lib/strategy/types';
import type { PositionsSummary } from 'types';
import {
  useStartEngineSimulationMutation,
  useGetEngineSimSummaryMutation,
} from '@lab/store/labEndpoints';
import { SimSummary } from '@lab/components/strategy/SimSummary';

type RunState = { running: boolean; summary?: PositionsSummary; error?: string };

/**
 * Full-corpus simulate for saved rules (lab app, FE3.2). Replaces the per-strategy
 * simulate flows with one generic surface: run a saved rule over the whole lake,
 * show its funnel summary inline. The dry-run panel (unsaved-draft loop) lives in
 * the rule editor; this page is for the persisted rules.
 */
export function SimulatePage() {
  const { data: rules = [], isLoading } = useGetStrategyRulesQuery();
  const { data: fps = [] } = useGetFingerprintsQuery();
  const [start] = useStartEngineSimulationMutation();
  const [fetchSummary] = useGetEngineSimSummaryMutation();
  const [runs, setRuns] = useState<Record<string, RunState>>({});
  const handleRef = useRef<{ close: () => void } | null>(null);

  const fpName = useMemo(() => new Map(fps.map((f) => [f.id, f.name || f.id.slice(0, 8)])), [fps]);

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

  const columns: ColumnDef<StrategyRule>[] = [
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
      render: (r) => (
        <span className="font-mono text-[12px] text-text-dim">
          {fpName.get(r.fingerprint_id) ?? r.fingerprint_id.slice(0, 8)}
        </span>
      ),
      searchValue: (r) => fpName.get(r.fingerprint_id) ?? r.fingerprint_id,
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
      key: 'result',
      label: 'Result',
      render: (r) => {
        const s = runs[r.id];
        if (!s) return <span className="text-text-dim/60">—</span>;
        if (s.running) return <span className="text-text-dim">running…</span>;
        if (s.error) return <span className="text-red">{s.error}</span>;
        if (s.summary) return <SimSummary summary={s.summary} />;
        return <span className="text-text-dim/60">cancelled</span>;
      },
      searchValue: () => '',
    },
  ];

  return (
    <div className="flex flex-col gap-3 p-4">
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
