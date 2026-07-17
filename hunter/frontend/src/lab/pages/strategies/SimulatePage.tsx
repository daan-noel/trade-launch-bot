import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';

import { DataTable } from 'components/table/DataTable';
import { RelativeTimeCell } from 'components/table/RelativeTimeCell';
import type { ColumnDef, TableQuery } from 'components/table/types';
import { Button } from 'components/ui/Button';
import { Badge } from 'components/ui/Badge';
import { InlineAlert } from 'components/ui/Modal';
import { SectionDivider } from 'components/ui/SectionDivider';
import { TokenTable } from 'components/tokens/TokenTable';
import { tokenNumericColKeys } from 'components/tokens/sharedTokenColumns';
import { dashF, dashPercent } from 'components/strategy/cellFormat';
import {
  inspectFromSim,
  markerRowOverlay,
  type InspectTarget,
} from 'components/strategy/inspectTarget';
import {
  matchedColumns,
  MATCHED_KEYS,
  simColumns,
  SIM_KEYS,
} from 'components/strategy/strategyColumns';
import { LabTokenInspectModal } from '@lab/components/strategy/LabTokenInspectModal';
import { apiErrorMessage } from 'store/baseApi';
import { connectSimulationFinished } from 'services/sse';
import {
  fetchEngineMatchedPage,
  fetchEngineSimPage,
  fetchEngineSimSummary,
} from 'services/api';
import {
  toSummaryBody,
  toTableRequest,
  type TableRequestBody,
} from 'services/tableRequest';
import { useGetFingerprintsQuery, useGetStrategyRulesQuery } from 'store/sharedEndpoints';
import { ruleParamsCell, ruleParamsSearchText } from 'components/strategy/RuleParamsSummary';
import {
  fingerprintParamsCell,
  fingerprintParamsSearchText,
} from 'components/strategy/FingerprintParamsSummary';
import { DEFAULT_POSITIONS_QUERY } from 'hooks/useRulePositions';
import { useServerTable } from 'hooks/useServerTable';
import { lamportsToSol, type Fingerprint, type StrategyRule, type TradeMode } from 'lib/strategy/types';
import type { MatchedTokenRecord, SimulatedSummary, SimulatedTokenResult } from 'types';
import {
  useStartEngineSimulationMutation,
  useGetEngineSimSummaryMutation,
} from '@lab/store/labEndpoints';

type RunState = { running: boolean; summary?: SimulatedSummary; error?: string };

const DASH = <span className="text-text-dim/60">—</span>;
const SIM_NUMERIC_COLS = tokenNumericColKeys(simColumns);
const MATCHED_NUMERIC_COLS = tokenNumericColKeys(matchedColumns);
const keyByMint = (r: { mint_address: string }) => r.mint_address;
const simRowOverlay = markerRowOverlay(inspectFromSim);

/**
 * Full-corpus simulate for saved rules (lab app, FE3.2). Replaces the per-strategy
 * simulate flows with one generic surface: run a saved rule over the whole lake,
 * show every rule's funnel summary as sortable/filterable columns, and list the
 * per-token positions of the selected rule below. The dry-run panel (unsaved-draft
 * loop) lives in the rule editor; this page is for persisted rules.
 */
export function SimulatePage() {
  const { data: rules = [], isLoading } = useGetStrategyRulesQuery();
  const { data: fps = [] } = useGetFingerprintsQuery();
  const [start] = useStartEngineSimulationMutation();
  const [fetchSummary] = useGetEngineSimSummaryMutation();
  const [runs, setRuns] = useState<Record<string, RunState>>({});
  const [bulkMode, setBulkMode] = useState<TradeMode | null>(null);
  const [selectedRuleId, setSelectedRuleId] = useState<string | null>(null);
  const [inspect, setInspect] = useState<{
    key: string;
    target: InspectTarget;
    rule: StrategyRule;
  } | null>(null);
  const [reloadNonce, setReloadNonce] = useState(0);
  const handleRef = useRef<{ close: () => void } | null>(null);
  const hydratedIds = useRef<Set<string>>(new Set());

  const fpById = useMemo(() => new Map(fps.map((f) => [f.id, f])), [fps]);

  const paperCount = useMemo(() => rules.filter((r) => r.trade_mode === 'paper').length, [rules]);
  const realCount = useMemo(() => rules.filter((r) => r.trade_mode === 'real').length, [rules]);

  // Hydrate every rule's resident sim summary on load (and when new rules appear),
  // so the table columns + position panels show without needing a row click.
  useEffect(() => {
    if (rules.length === 0) return;
    let cancelled = false;
    const pending = rules.filter((r) => !hydratedIds.current.has(r.id));
    if (pending.length === 0) return;

    void (async () => {
      await Promise.all(
        pending.map(async (rule) => {
          hydratedIds.current.add(rule.id);
          try {
            const summary = await fetchSummary(rule.id).unwrap();
            if (cancelled) return;
            setRuns((r) => {
              if (r[rule.id]?.running) return r;
              return { ...r, [rule.id]: { running: false, summary } };
            });
          } catch {
            /* no resident result for this rule */
          }
        }),
      );
    })();

    return () => {
      cancelled = true;
    };
  }, [rules, fetchSummary]);

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
        hydratedIds.current.add(id);
        setRuns((r) => ({ ...r, [id]: { running: false, summary } }));
      } catch (e) {
        setRuns((r) => ({ ...r, [id]: { running: false, error: apiErrorMessage(e as never) ?? 'summary failed' } }));
      }
      setReloadNonce((n) => n + 1);
    });
    return () => handleRef.current?.close();
  }, [fetchSummary]);

  const runRule = async (rule: StrategyRule) => {
    setRuns((r) => ({ ...r, [rule.id]: { running: true } }));
    setSelectedRuleId(rule.id);
    try {
      await start({ rule_id: rule.id }).unwrap();
    } catch (e) {
      setRuns((r) => ({ ...r, [rule.id]: { running: false, error: apiErrorMessage(e as never) ?? 'start failed' } }));
    }
  };

  /** Queue a lake backtest for every saved rule in `mode` that isn't already running. */
  const runAll = async (mode: TradeMode) => {
    const targets = rules.filter((r) => r.trade_mode === mode && !runs[r.id]?.running);
    if (targets.length === 0) return;
    setBulkMode(mode);
    setRuns((prev) => {
      const next = { ...prev };
      for (const r of targets) next[r.id] = { running: true };
      return next;
    });
    try {
      await Promise.all(
        targets.map(async (rule) => {
          try {
            await start({ rule_id: rule.id }).unwrap();
          } catch (e) {
            setRuns((r) => ({
              ...r,
              [rule.id]: { running: false, error: apiErrorMessage(e as never) ?? 'start failed' },
            }));
          }
        }),
      );
    } finally {
      setBulkMode(null);
    }
  };

  const columns = useMemo(() => buildColumns(runs, fpById), [runs, fpById]);

  // Only the selected rule's positions render below the table; every rule's summary
  // is still hydrated for the table's metric columns.
  const selectedRule = useMemo(
    () => rules.find((r) => r.id === selectedRuleId) ?? null,
    [rules, selectedRuleId],
  );
  const selectedRun = selectedRuleId ? runs[selectedRuleId] : undefined;

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-3">
        <div className="flex flex-wrap items-baseline gap-3">
          <h1 className="text-lg font-semibold text-text">Simulate</h1>
          <span className="text-sm text-text-mid">
            Lake backtest for saved rules · drafts use Rules dry-run
          </span>
        </div>
        <div className="ml-auto flex flex-wrap items-center gap-2">
          <Button
            variant="subtle"
            size="sm"
            disabled={paperCount === 0 || bulkMode !== null}
            onClick={() => runAll('paper')}
          >
            {bulkMode === 'paper' ? 'Starting paper…' : `Simulate All Paper (${paperCount})`}
          </Button>
          <Button
            variant="subtle"
            size="sm"
            disabled={realCount === 0 || bulkMode !== null}
            onClick={() => runAll('real')}
          >
            {bulkMode === 'real' ? 'Starting real…' : `Simulate All Real (${realCount})`}
          </Button>
        </div>
      </div>
      <DataTable
        columns={columns}
        rows={rules}
        rowKey={(r) => r.id}
        loading={isLoading}
        searchable
        tableId="simulate-rules"
        emptyMessage="No rules yet — author one on the Rules page."
        selectedKey={selectedRuleId}
        onSelect={setSelectedRuleId}
        rowActions={(r) => (
          <Button
            variant="primary"
            size="xs"
            disabled={runs[r.id]?.running}
            onClick={() => void runRule(r)}
          >
            {runs[r.id]?.running ? 'Running…' : 'Simulate'}
          </Button>
        )}
      />

      {selectedRule && <SectionDivider gap="md" />}

      {selectedRule && selectedRun?.running && (
        <p className="text-sm text-text-dim">
          Simulating <span className="font-medium text-text">{selectedRule.rule_name}</span>…
        </p>
      )}

      {selectedRule && !selectedRun?.running && (
        <RuleSimPositionsPanel
          key={selectedRule.id}
          rule={selectedRule}
          reloadNonce={reloadNonce}
          onInspect={setInspect}
          inspectKey={inspect?.key ?? null}
        />
      )}

      {inspect && (
        <LabTokenInspectModal
          target={inspect.target}
          titleSuffix="Sim inspect"
          ruleOverride={{
            paramsJson: inspect.rule.params,
            fingerprintId: inspect.rule.fingerprint_id,
            label: inspect.rule.rule_name,
          }}
          onClose={() => setInspect(null)}
        />
      )}
    </div>
  );
}

type ResultView = 'positions' | 'matched';

/** The selected rule's result detail — a Positions ⇄ Matched toggle over two
 *  server-side tables: the entered **positions** (the sim run's outcomes) and the
 *  fingerprint-**matched** candidate pool they're a subset of. Rendered only for the
 *  row the user picked in the rules table above; each table fetches only while its
 *  view is active. */
function RuleSimPositionsPanel({
  rule,
  reloadNonce,
  onInspect,
  inspectKey,
}: {
  rule: StrategyRule;
  reloadNonce: number;
  onInspect: (v: { key: string; target: InspectTarget; rule: StrategyRule } | null) => void;
  inspectKey: string | null;
}) {
  const [view, setView] = useState<ResultView>('positions');

  // Positions (entered) — the sim run's per-token outcomes. Fetched only while the
  // Positions view is active.
  const [simQuery, setSimQuery] = useState<TableQuery>(DEFAULT_POSITIONS_QUERY);
  const simBody = useMemo(() => toTableRequest(simQuery, SIM_NUMERIC_COLS), [simQuery]);
  const simSummaryBody = useMemo(
    () => toSummaryBody(simQuery, SIM_NUMERIC_COLS),
    [simQuery.search, simQuery.colFilters, simQuery.structuredFilters],
  );
  const {
    items: simTokens,
    total: simTotal,
    loading: simTableLoading,
    error: simTableError,
    reload: reloadSim,
  } = useServerTable<SimulatedTokenResult, SimulatedSummary>(
    view === 'positions',
    simBody,
    (body, signal) => fetchEngineSimPage(rule.id, body as TableRequestBody, signal),
    (summaryBody, signal) =>
      fetchEngineSimSummary(rule.id, summaryBody as TableRequestBody, signal),
    simSummaryBody,
  );

  // Matched — every token the rule's fingerprint selects (positions are the subset
  // that actually entered). No summary and no entry/exit overlay: these are
  // candidates, not fills. Fetched only while the Matched view is active.
  const [matchedQuery, setMatchedQuery] = useState<TableQuery>(DEFAULT_POSITIONS_QUERY);
  const matchedBody = useMemo(
    () => toTableRequest(matchedQuery, MATCHED_NUMERIC_COLS),
    [matchedQuery],
  );
  const {
    items: matchedTokens,
    total: matchedTotal,
    loading: matchedLoading,
    error: matchedError,
    reload: reloadMatched,
  } = useServerTable<MatchedTokenRecord>(view === 'matched', matchedBody, (body, signal) =>
    fetchEngineMatchedPage(rule.id, body as TableRequestBody, signal),
  );

  useEffect(() => {
    reloadSim();
    reloadMatched();
  }, [reloadNonce, reloadSim, reloadMatched]);

  const onSelectSim = useCallback(
    (key: string | null) => {
      const row = key ? simTokens.find((t) => t.mint_address === key) ?? null : null;
      onInspect(row ? { key: row.mint_address, target: inspectFromSim(row), rule } : null);
    },
    [simTokens, onInspect, rule],
  );

  const isMatched = view === 'matched';
  // "no simulation result" just means this rule has no resident run — the parent
  // only mounts us once it has a summary, so show the empty table, not an error.
  const simError = simTableError && !simTableError.includes('no simulation result')
    ? simTableError
    : null;

  return (
    <section id={`sim-positions-${rule.id}`}>
      <div className="mb-3.5 flex flex-wrap items-center gap-2.5">
        <span className="h-4 w-1 rounded-full bg-info" />
        <h3 className="text-sm font-bold text-text">
          {isMatched ? 'Matched Tokens' : 'Simulated Positions'}
        </h3>
        <Badge variant="info" size="sm" className="font-mono font-normal">
          {isMatched ? matchedTotal : simTotal}
        </Badge>
        <div className="flex items-center gap-1">
          <Button
            variant={isMatched ? 'subtle' : 'primary'}
            size="xs"
            onClick={() => setView('positions')}
          >
            Positions
          </Button>
          <Button
            variant={isMatched ? 'primary' : 'subtle'}
            size="xs"
            onClick={() => setView('matched')}
          >
            Matched
          </Button>
        </div>
        <span className="truncate font-mono text-[11px] text-text-dim">{rule.rule_name}</span>
      </div>

      {isMatched ? (
        matchedError ? (
          <InlineAlert variant="error">{matchedError}</InlineAlert>
        ) : (
          <TokenTable
            columns={matchedColumns}
            existingKeys={MATCHED_KEYS}
            mintSetFilter
            charts
            rows={matchedTokens}
            rowKey={keyByMint}
            serverSide
            serverTotal={matchedTotal}
            onQueryChange={setMatchedQuery}
            loading={matchedLoading}
            resetKey={rule.id}
            tableId={`simulate-matched-${rule.id}`}
            emptyMessage="No tokens match this rule's fingerprint."
          />
        )
      ) : simError ? (
        <InlineAlert variant="error">{simError}</InlineAlert>
      ) : (
        <TokenTable
          columns={simColumns}
          existingKeys={SIM_KEYS}
          mintSetFilter
          charts
          useRowOverlay={simRowOverlay}
          rows={simTokens}
          rowKey={keyByMint}
          selectedKey={inspectKey}
          onSelect={onSelectSim}
          serverSide
          serverTotal={simTotal}
          onQueryChange={setSimQuery}
          loading={simTableLoading}
          resetKey={rule.id}
          tableId={`simulate-positions-${rule.id}`}
          emptyMessage="No positions in this simulation result."
        />
      )}
    </section>
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
      group: 'name',
      render: (r) => (
        <div className="flex min-w-40 flex-col gap-0.5">
          <span className="font-medium text-text">{r.rule_name}</span>
          <span className="text-[10px] text-text-dim">
            {r.is_active ? 'armed on live' : 'idle on live'}
          </span>
        </div>
      ),
      searchValue: (r) => `${r.rule_name} ${r.is_active ? 'active' : 'idle'}`,
    },
    {
      key: 'mode',
      label: 'Mode',
      group: 'status',
      render: (r) => (
        <Badge variant={r.trade_mode === 'real' ? 'warning' : 'info'}>{r.trade_mode}</Badge>
      ),
      searchValue: (r) => r.trade_mode,
    },
    {
      key: 'fingerprint',
      label: 'Fingerprint',
      group: 'fingerprint',
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
      key: 'params',
      label: 'Params',
      group: 'params',
      render: (r) => ruleParamsCell(r.params),
      searchValue: (r) => ruleParamsSearchText(r.params),
    },
    {
      key: 'sim_run',
      label: 'Run',
      group: 'sim',
      tooltip: 'When this rule’s last simulation result was generated',
      render: (r) => {
        const run = runOf(r);
        if (!run) return DASH;
        if (run.running) return <span className="text-text-dim">running…</span>;
        if (run.error) return <span className="text-red">{run.error}</span>;
        if (run.summary) {
          return run.summary.computed_at ? (
            <RelativeTimeCell iso={run.summary.computed_at} />
          ) : (
            <span className="text-text-dim">done</span>
          );
        }
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
