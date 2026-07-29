import { useCallback, useEffect, useMemo, useState } from 'react';

import { Badge } from 'components/ui/Badge';
import { InlineAlert } from 'components/ui/Modal';
import { VisibilityToggleButton } from 'components/ui/VisibilityToggleButton';
import { TokenTable } from 'components/tokens/TokenTable';
import { tokenAmountColKeys, tokenNumericColKeys } from 'components/tokens/sharedTokenColumns';
import { simColumns, SIM_KEYS } from 'components/strategy/strategyColumns';
import {
  episodeRowKey,
  inspectFromSim,
  markerRowOverlay,
  type InspectTarget,
} from 'components/strategy/inspectTarget';
import type { RuleEditorDraft } from 'components/strategy/RuleEditor';
import { TemporalSummary, type TemporalSelection } from 'components/strategy/TemporalSummary';
import { LazyLabTokenInspectModal } from '@lab/components/strategy/LazyLabTokenInspectModal';
import {
  useSimInspectEpisodeMarkers,
  useSimMintEpisodeOverlay,
} from '@lab/hooks/useSimMintEpisodeOverlay';
import { fetchEngineSimPage, fetchEngineSimSummary, fetchEngineSimTimeSummary } from 'services/api';
import { toSummaryBody, toTableRequest, type TableRequestBody } from 'services/tableRequest';
import { DEFAULT_POSITIONS_QUERY, useServerTable } from 'hooks/useServerTable';
import { patternKeysFrom } from 'lib/flow/classifyFlow';
import { volumeIxPatternsFromConfig } from 'lib/strategy/registry';
import { useGetFingerprintsQuery } from 'store/sharedEndpoints';
import type { TableQuery } from 'components/table/types';
import type {
  HoldSchemeChoice,
  WallGrainChoice,
  WallTimeField,
} from 'lib/strategy/temporalSummary';
import type { SimulatedSummary, SimulatedTokenResult, TemporalSummaryPayload } from 'types';
import { SimSummary } from './SimSummary';

const SIM_NUMERIC_COLS = tokenNumericColKeys(simColumns);
const SIM_AMOUNT_COLS = tokenAmountColKeys(simColumns);
const simRowOverlay = markerRowOverlay(inspectFromSim);

/**
 * Per-token detail for a finished dry-run (Tier 2): the exact `/result` +
 * `/result/time-summary` endpoints the Simulate page reads for saved rules, but
 * keyed by the inline draft's `run_id`. Renders trades (`simColumns`) with the
 * same chart entry/exit overlays + row inspect (metric panes) as Simulate,
 * pinned to the editor draft via `ruleOverride`.
 */
export function DryRunDetail({
  runId,
  draft,
}: {
  runId: string;
  draft: RuleEditorDraft;
}) {
  const { data: fingerprints = [] } = useGetFingerprintsQuery();
  const flowPatternKeys = useMemo(() => {
    const fp = fingerprints.find((f) => f.id === draft.fingerprint_id);
    if (!fp) return null;
    return patternKeysFrom(volumeIxPatternsFromConfig(fp.metric_config));
  }, [fingerprints, draft.fingerprint_id]);

  const [simQuery, setSimQuery] = useState<TableQuery>(DEFAULT_POSITIONS_QUERY);
  // Matched-but-not-entered `NoEntry` rows are hidden by default — a dry-run's
  // "Trades" read is the positions it took; toggle to see everything it matched.
  const [showNotFired, setShowNotFired] = useState(false);
  const [temporalSel, setTemporalSel] = useState<TemporalSelection>(null);
  const [wallField, setWallField] = useState<WallTimeField>('created_at');
  const [wallGrain, setWallGrain] = useState<WallGrainChoice>('auto');
  const [holdScheme, setHoldScheme] = useState<HoldSchemeChoice>('auto');
  const [timeSummary, setTimeSummary] = useState<TemporalSummaryPayload | null>(null);
  const [inspect, setInspect] = useState<{ key: string; target: InspectTarget } | null>(null);
  const episodeMarkers = useSimInspectEpisodeMarkers(runId, inspect?.target ?? null);

  // Hide-not-fired injects a server-side `exit_reason != NoEntry` so paging /
  // totals stay correct (same toggle as the sweep combo drill-in).
  const applyNotFired = useCallback(
    (q: TableQuery): TableQuery =>
      showNotFired
        ? q
        : {
            ...q,
            structuredFilters: {
              ...q.structuredFilters,
              exit_reason: { op: 'neq', val: 'NoEntry' },
            },
          },
    [showNotFired],
  );

  // A temporal-bin click narrows the table page to that cohort's mints.
  const pageQuery = useMemo(() => {
    const base = applyNotFired(simQuery);
    if (!temporalSel?.mints.length) return base;
    return {
      ...base,
      structuredFilters: {
        ...base.structuredFilters,
        mint_address: { op: 'in' as const, val: temporalSel.mints },
      },
    };
  }, [simQuery, temporalSel, applyNotFired]);

  const simBody = useMemo(
    () => toTableRequest(pageQuery, SIM_NUMERIC_COLS, { amountCols: SIM_AMOUNT_COLS }),
    [pageQuery],
  );
  // Time chart stays on the table's own filters (not the bin click) so the
  // driving chart doesn't collapse after a selection.
  const timeBody = useMemo(
    () => toSummaryBody(applyNotFired(simQuery), SIM_NUMERIC_COLS, { amountCols: SIM_AMOUNT_COLS }),
    [simQuery.search, simQuery.colFilters, simQuery.structuredFilters, applyNotFired],
  );

  // KPI summary tracks the page cohort (search/column filters + temporal click),
  // so the dry-run tiles update as the trades table is filtered — same as Simulate.
  const simSummaryBody = useMemo(
    () => toSummaryBody(pageQuery, SIM_NUMERIC_COLS, { amountCols: SIM_AMOUNT_COLS }),
    [pageQuery],
  );

  const fetchPage = useCallback(
    (body: unknown, signal: AbortSignal) =>
      fetchEngineSimPage(runId, body as TableRequestBody, signal),
    [runId],
  );
  const fetchSummary = useCallback(
    (body: unknown, signal: AbortSignal) =>
      fetchEngineSimSummary(runId, body as TableRequestBody, signal),
    [runId],
  );
  const {
    items: rows,
    total,
    summary,
    loading,
    error,
  } = useServerTable<SimulatedTokenResult, SimulatedSummary>(
    true,
    simBody,
    fetchPage,
    fetchSummary,
    simSummaryBody,
    runId,
  );

  // Clear the temporal cohort when the table's own filters change or the run switches.
  const baseFilterKey = JSON.stringify({
    s: simQuery.search,
    c: simQuery.colFilters,
    f: simQuery.structuredFilters,
  });
  useEffect(() => {
    setTemporalSel(null);
  }, [runId, baseFilterKey]);

  useEffect(() => {
    setInspect(null);
  }, [runId]);

  useEffect(() => {
    const ctrl = new AbortController();
    void fetchEngineSimTimeSummary(
      runId,
      timeBody as TableRequestBody,
      wallField,
      wallGrain,
      holdScheme,
      ctrl.signal,
    )
      .then((t) => {
        if (!ctrl.signal.aborted) setTimeSummary(t);
      })
      .catch((e) => {
        if (e instanceof DOMException && e.name === 'AbortError') return;
        if (!ctrl.signal.aborted) setTimeSummary(null);
      });
    return () => ctrl.abort();
  }, [runId, timeBody, wallField, wallGrain, holdScheme]);

  const onSelect = useCallback(
    (key: string | null) => {
      const row = key
        ? rows.find((t) => episodeRowKey(t) === key) ??
          rows.find((t) => t.mint_address === key) ??
          null
        : null;
      setInspect(row ? { key: episodeRowKey(row), target: inspectFromSim(row) } : null);
    },
    [rows],
  );

  const ruleOverride = useMemo(
    () => ({
      paramsJson: draft.params,
      fingerprintId: draft.fingerprint_id,
      label: draft.rule_name.trim() || 'dry-run draft',
    }),
    [draft.params, draft.fingerprint_id, draft.rule_name],
  );

  return (
    <section className="mt-3">
      <div className="mb-3 flex flex-wrap items-center gap-2.5">
        <span className="h-4 w-1 rounded-full bg-info" />
        <h3 className="text-sm font-bold text-text">Trades</h3>
        <Badge variant="info" size="sm" className="font-mono font-normal">
          {total}
        </Badge>
        <VisibilityToggleButton
          visible={showNotFired}
          onToggle={() => {
            setShowNotFired((v) => !v);
            setSimQuery((q) => ({ ...q, page: 1 }));
          }}
          label="not-fired tokens"
        >
          {showNotFired ? 'Hide not fired' : 'Show not fired'}
        </VisibilityToggleButton>
      </div>

      {error ? (
        <InlineAlert variant="error">{error}</InlineAlert>
      ) : (
        <>
          {summary && <SimSummary summary={summary} />}
          {timeSummary && timeSummary.nFired > 0 && (
            <TemporalSummary
              data={{
                ...timeSummary,
                holdScheme: timeSummary.holdScheme ?? 'mid_30m',
                holdSchemeAuto:
                  timeSummary.holdSchemeAuto ?? timeSummary.holdScheme ?? 'mid_30m',
                wallGrainAuto: timeSummary.wallGrainAuto ?? timeSummary.wallGrain,
                wallSpanMs: timeSummary.wallSpanMs ?? 0,
              }}
              selection={temporalSel}
              onSelect={setTemporalSel}
              wallField={wallField}
              onWallFieldChange={setWallField}
              wallGrain={wallGrain}
              onWallGrainChange={setWallGrain}
              holdScheme={holdScheme}
              onHoldSchemeChange={setHoldScheme}
            />
          )}
          <TokenTable
            columns={simColumns}
            existingKeys={SIM_KEYS}
            mintSetFilter
            rows={rows}
            rowKey={episodeRowKey}
            charts
            useRowOverlay={simRowOverlay}
            chartsGroupByMint
            useMintChartGroupOverlay={(mint, pageRows) =>
              useSimMintEpisodeOverlay(runId, mint, pageRows)
            }
            flowPatternKeys={flowPatternKeys}
            selectedKey={inspect?.key ?? null}
            onSelect={onSelect}
            serverSide
            serverTotal={total}
            onQueryChange={setSimQuery}
            loading={loading}
            resetKey={`${runId}_${showNotFired}`}
            tableId="dryrun-positions"
            emptyMessage={
              showNotFired
                ? 'No tokens in this dry-run result.'
                : 'No fired positions in this dry-run result.'
            }
          />
        </>
      )}

      {inspect && (
        <LazyLabTokenInspectModal
          target={inspect.target}
          titleSuffix="Dry-run inspect"
          ruleOverride={ruleOverride}
          eventMarkers={episodeMarkers}
          onClose={() => setInspect(null)}
        />
      )}
    </section>
  );
}
