import { useCallback, useEffect, useMemo, useState } from 'react';
import { Link, useParams } from 'react-router-dom';
import { useSelector } from 'react-redux';
import { TokenTable } from 'components/tokens/TokenTable';
import { InlineAlert } from 'components/ui/Modal';
import { Badge } from 'components/ui/Badge';
import { SimSummaryCard } from 'components/strategy/SimSummaryCard';
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
import { useGetStrategyRulesQuery } from 'store/sharedEndpoints';
import { rulesHref } from 'lib/strategy/nav';
import type { TableQuery } from 'components/table/types';
import type { PositionsSummary, RulePositionRecord } from 'types';
import { selectOpenByRule } from '@live/slices/liveStatusSlice';

const STRATEGY_SEG = 'generic';
const POS_NUMERIC = numericColKeys(positionColumns);
const posRowOverlay = markerRowOverlay(inspectFromPosition);

type Scope = 'current' | 'history';

/**
 * Per-rule Analyze — Positions Summary + paged traded history (current/history).
 * Open rows stay consistent with Live Status via SSE-triggered reload.
 */
export function RuleAnalyzePage() {
  const { ruleId = '' } = useParams<{ ruleId: string }>();
  const { data: rules = [] } = useGetStrategyRulesQuery();
  const rule = rules.find((r) => r.id === ruleId);
  const price = usePriceDisplay();
  const [scope, setScope] = useState<Scope>('current');
  const [query, setQuery] = useState<TableQuery>(DEFAULT_POSITIONS_QUERY);

  const liveOpen = useSelector(selectOpenByRule(ruleId));

  const body = useMemo(
    () => toTableRequest(query, POS_NUMERIC),
    [query],
  );
  const summaryBody = useMemo(
    () => toSummaryBody(query, POS_NUMERIC),
    [query],
  );

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
  >(!!ruleId, body, fetchPage, fetchSummary, summaryBody);

  // Any position delta for this rule → reload page + summary (open patch + closed appear).
  useEffect(() => {
    if (!ruleId) return;
    const h = connectStrategyPositionUpdate((d) => {
      if (d.rule_id === ruleId) reload();
    });
    return () => h.close();
  }, [ruleId, reload]);

  if (!ruleId) {
    return <InlineAlert variant="error">Missing rule id.</InlineAlert>;
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-baseline justify-between gap-3">
        <div className="flex flex-wrap items-baseline gap-3">
          <Link
            to={rulesHref(ruleId)}
            className="text-sm text-accent hover:text-primary hover:underline"
          >
            ← Rules
          </Link>
          <h1 className="text-lg font-extrabold text-text">
            {rule?.rule_name ?? ruleId.slice(0, 8)}
          </h1>
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
        <div className="flex gap-1">
          {(['current', 'history'] as Scope[]).map((s) => (
            <button
              key={s}
              type="button"
              onClick={() => {
                setScope(s);
                setQuery((q) => ({ ...q, page: 1 }));
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
      </div>

      {error && <InlineAlert variant="error">{error}</InlineAlert>}

      {summary && (
        <SimSummaryCard
          ruleName={rule?.rule_name ?? ruleId.slice(0, 8)}
          price={price}
          title="Positions Summary"
          summary={summary}
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
        onQueryChange={setQuery}
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
