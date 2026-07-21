import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useSelector } from 'react-redux';
import { TokenTable } from 'components/tokens/TokenTable';
import { InlineAlert } from 'components/ui/Modal';
import { Badge } from 'components/ui/Badge';
import { SimSummaryCard } from 'components/strategy/SimSummaryCard';
import { TemporalSummary, type TemporalSelection } from 'components/strategy/TemporalSummary';
import {
  positionColumns,
  POSITION_KEYS,
} from 'components/strategy/strategyColumns';
import { inspectFromPosition, markerRowOverlay } from 'components/strategy/inspectTarget';
import { usePriceDisplay } from 'hooks/usePriceDisplay';
import { useServerTable, DEFAULT_POSITIONS_QUERY } from 'hooks/useServerTable';
import { numericColKeys, toSummaryBody, toTableRequest } from 'services/tableRequest';
import {
  fetchRulePositionsPage,
  fetchRulePositionsSummary,
} from 'services/api';
import { connectStrategyPositionUpdate } from 'services/sse';
import type { TableQuery } from 'components/table/types';
import type { PositionsSummary, RulePositionRecord } from 'types';
import type { StrategyRule } from 'lib/strategy/types';
import type { TemporalRow } from 'lib/strategy/temporalSummary';
import { selectOpenByRule } from '@live/slices/liveStatusSlice';

const STRATEGY_SEG = 'generic';
const POS_NUMERIC = numericColKeys(positionColumns);
const posRowOverlay = markerRowOverlay(inspectFromPosition);

type Scope = 'current' | 'history';

const TABLE_RELOAD_STATUSES = new Set([
  'Holding',
  'End',
  'ExitFailed',
  'ExitUnconfirmed',
]);

function holdingSecs(r: RulePositionRecord): number {
  if (!r.entry_time) return 0;
  const start = Date.parse(r.entry_time);
  if (!Number.isFinite(start)) return 0;
  const end = r.exit_time ? Date.parse(r.exit_time) : Date.now();
  if (!Number.isFinite(end)) return 0;
  return Math.max(0, (end - start) / 1000);
}

function toTemporalRow(r: RulePositionRecord): TemporalRow {
  const open = !r.exit_time && r.status !== 'End' && r.status !== 'ExitFailed';
  return {
    mint_address: r.mint_address,
    fired: r.entry_price != null,
    exit: open ? 'Open' : (r.exit_reason ?? r.status),
    pnl_sol: r.pnl_sol ?? 0,
    holding_secs: holdingSecs(r),
    entry_time: r.entry_time,
    created_at: r.created_at,
  };
}

export interface RuleAnalyzePanelProps {
  ruleId: string;
  rule?: StrategyRule | null;
  /** Compact header when embedded under the Rules table. */
  embedded?: boolean;
  onClose?: () => void;
}

/**
 * Per-rule Analyze body — Positions Summary + temporal bands + paged history.
 * Used embedded on Rules (master–detail) and as the standalone Analyze route.
 */
export function RuleAnalyzePanel({
  ruleId,
  rule,
  embedded,
  onClose,
}: RuleAnalyzePanelProps) {
  const price = usePriceDisplay();
  const [scope, setScope] = useState<Scope>('current');
  const [query, setQuery] = useState<TableQuery>(DEFAULT_POSITIONS_QUERY);
  const [temporalSel, setTemporalSel] = useState<TemporalSelection>(null);
  /** True after we auto-flipped Current→History for an empty current run. */
  const [autoOpenedHistory, setAutoOpenedHistory] = useState(false);
  /** One auto-flip attempt per rule select (don't fight a manual Current click). */
  const autoHistoryTried = useRef(false);

  const liveOpen = useSelector(selectOpenByRule(ruleId));

  const body = useMemo(() => {
    const base = toTableRequest(query, POS_NUMERIC);
    if (!temporalSel?.mints.length) return base;
    return {
      ...base,
      filters: {
        ...base.filters,
        mint_address: { op: 'in' as const, val: temporalSel.mints },
      },
    };
  }, [query, temporalSel]);

  const summaryBody = useMemo(() => {
    const base = toSummaryBody(query, POS_NUMERIC);
    if (!temporalSel?.mints.length) return base;
    return {
      ...base,
      filters: {
        ...base.filters,
        mint_address: { op: 'in' as const, val: temporalSel.mints },
      },
    };
  }, [query, temporalSel]);

  const fetchPage = useCallback(
    (b: unknown, signal: AbortSignal) =>
      fetchRulePositionsPage(STRATEGY_SEG, ruleId, b as never, scope, signal),
    [ruleId, scope],
  );
  const fetchSummary = useCallback(
    (b: unknown, signal: AbortSignal) =>
      fetchRulePositionsSummary(STRATEGY_SEG, ruleId, b as never, scope, signal),
    [ruleId, scope],
  );

  const { items, total, summary, loading, error, reload } = useServerTable<
    RulePositionRecord,
    PositionsSummary
  >(!!ruleId, body, fetchPage, fetchSummary, summaryBody, `${ruleId}:${scope}`);

  useEffect(() => {
    setTemporalSel(null);
    setScope('current');
    setQuery(DEFAULT_POSITIONS_QUERY);
    setAutoOpenedHistory(false);
    autoHistoryTried.current = false;
  }, [ruleId]);

  // Real scoreboard N is all-time; Analyze defaults to the latest run. After
  // Stop→Activate the current run is empty while N still counts priors — flip
  // to History once so Positions Summary matches what the operator clicked for.
  useEffect(() => {
    if (loading || autoHistoryTried.current) return;
    if (scope !== 'current' || total > 0) return;
    if (rule?.trade_mode !== 'real' || (rule.total_positions ?? 0) <= 0) return;
    autoHistoryTried.current = true;
    setAutoOpenedHistory(true);
    setScope('history');
    setTemporalSel(null);
    setQuery((q) => ({ ...q, page: 1 }));
  }, [loading, scope, total, rule]);

  useEffect(() => {
    if (!ruleId) return;
    const h = connectStrategyPositionUpdate((d) => {
      if (d.rule_id !== ruleId) return;
      if (!TABLE_RELOAD_STATUSES.has(d.status)) return;
      reload();
    });
    return () => h.close();
  }, [ruleId, reload]);

  const temporalRows = useMemo(() => items.map(toTemporalRow), [items]);

  if (!ruleId) {
    return <InlineAlert variant="error">Missing rule id.</InlineAlert>;
  }

  return (
    <div className={`flex flex-col gap-4 ${embedded ? 'rounded-lg border border-white/8 bg-panel/40 p-4' : ''}`}>
      <div className="flex flex-wrap items-baseline justify-between gap-3">
        <div className="flex flex-wrap items-baseline gap-3">
          {embedded ? (
            <h2 className="text-base font-extrabold text-text">
              {rule?.rule_name ?? ruleId.slice(0, 8)}
            </h2>
          ) : (
            <h1 className="text-lg font-extrabold text-text">
              {rule?.rule_name ?? ruleId.slice(0, 8)}
            </h1>
          )}
          {rule && (
            <Badge variant={rule.trade_mode === 'real' ? 'warning' : 'info'}>
              {rule.trade_mode}
            </Badge>
          )}
          {rule && (
            <Badge variant={rule.is_active ? 'success' : 'neutral'}>
              {rule.is_active ? 'Active' : 'Idle'}
            </Badge>
          )}
          <span className="text-sm text-text-mid">
            Analyze · {liveOpen.length} open live
          </span>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <div className="flex gap-1">
            {(['current', 'history'] as Scope[]).map((s) => (
              <button
                key={s}
                type="button"
                onClick={() => {
                  setAutoOpenedHistory(false);
                  setScope(s);
                  setQuery((q) => ({ ...q, page: 1 }));
                  setTemporalSel(null);
                }}
                className={`rounded-md px-2.5 py-1 text-xs font-semibold capitalize ${
                  scope === s
                    ? 'bg-primary/20 text-primary'
                    : 'bg-white/5 text-text-dim hover:bg-white/8'
                }`}
              >
                {s === 'current' ? 'Current run' : 'History'}
              </button>
            ))}
          </div>
          {onClose && (
            <button
              type="button"
              onClick={onClose}
              className="rounded-md px-2 py-1 text-xs text-text-dim hover:bg-white/8 hover:text-text"
            >
              Close
            </button>
          )}
        </div>
      </div>

      {error && <InlineAlert variant="error">{error}</InlineAlert>}

      {autoOpenedHistory && scope === 'history' && (
        <InlineAlert variant="warning">
          Current run is empty — showing History. Scoreboard N={rule?.total_positions}{' '}
          is all-time for real rules.
        </InlineAlert>
      )}

      {summary && (
        <SimSummaryCard
          ruleName={rule?.rule_name ?? ruleId.slice(0, 8)}
          price={price}
          title="Positions Summary"
          summary={summary}
        />
      )}

      {temporalRows.length > 0 && (
        <TemporalSummary
          rows={temporalRows}
          selection={temporalSel}
          onSelect={setTemporalSel}
          wallField="entry_time"
        />
      )}

      <TokenTable
        columns={positionColumns}
        existingKeys={POSITION_KEYS}
        rows={items}
        rowKey={(r) => r.id}
        loading={loading}
        serverSide
        serverTotal={total}
        onQueryChange={(q) => {
          setTemporalSel(null);
          setQuery(q);
        }}
        useRowOverlay={posRowOverlay}
        charts
        resetKey={`${ruleId}_${scope}`}
        tableId={`rule-analyze-${ruleId}`}
        emptyMessage={
          scope === 'history'
            ? 'No positions in prior runs.'
            : 'No positions in the current run yet.'
        }
      />
    </div>
  );
}
