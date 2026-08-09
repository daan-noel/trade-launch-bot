/**
 * Portfolio drill-down — the selected rule's **closed** positions for the page's
 * calendar window, with the per-row charts grid and the position inspect modal.
 *
 * It answers "which trades produced that number?" without leaving the board, and
 * it is scoped to exactly the population the rule's scoreboard row aggregates:
 * entered, `status = 'End'`, inside the same window. So the panel's row count is
 * the row's **N** and its PnL ◎ column sums to the row's **PnL** — a drill-down
 * that didn't reconcile would quietly be answering a different question than the
 * number that was clicked.
 *
 * That scope is **not re-derived here**. The cohort is a `HistoryCohort` with
 * `lane: 'closed'`, serialized by the one request builder Console History uses
 * (`historyRequest`), so the two surfaces cannot disagree about what "closed in
 * this window" means, and the `?rule=` panel and the row's "History" deep link
 * land on the same trades.
 */

import { memo, useCallback, useMemo, useState } from 'react';
import { TokenTable } from 'components/tokens/TokenTable';
import { ALL_TOKEN_INFO_KEYS } from 'components/tokens/sharedTokenColumns';
import { inspectFromPosition, markerRowOverlay } from 'components/strategy/inspectTarget';
import {
  PositionChartCardExtra,
  positionChartFactsFromRule,
} from 'components/strategy/PositionChartCardExtra';
import { InlineAlert } from 'components/ui/Modal';
import { CloseIcon } from 'components/ui/icons';
import type { TableQuery } from 'components/table/types';
import { numericColKeys } from 'services/tableRequest';
import { fetchPortfolioPositionsPage } from 'services/api';
import { DEFAULT_POSITIONS_QUERY, useServerTable } from 'hooks/useServerTable';
import { formatSigned, signedToneClass } from 'lib/signedTone';
import { useTimezone } from 'context/TimezoneContext';
import type { PortfolioRulePnl, RulePositionRecord } from 'types';
import { usePositionArrowNav } from '@live/components/floor/usePositionArrowNav';
import { LazyLivePositionInspectModal } from '@live/components/strategy/LazyLivePositionInspectModal';
import { historyColumns } from '@live/components/history/HistoryTable';
import { presetStart, type HistoryCohort } from '@live/pages/console/historyCohort';
import { historyCohortKey, historyTableBody } from '@live/pages/console/historyRequest';

export type PortfolioRange = 'today' | '7d' | '30d' | 'all';

const RANGE_LABEL: Record<PortfolioRange, string> = {
  today: 'today',
  '7d': 'last 7 days',
  '30d': 'last 30 days',
  all: 'all-time',
};

/**
 * Console History's columns minus the two this cohort holds constant: every row
 * is the selected rule (`rule`) and every row is closed in the selected mode
 * (`status`). A column that renders the same value in every row spends width to
 * say nothing, and offers a sort that can't reorder anything.
 */
const OMIT_WHEN_SCOPED = new Set(['rule', 'status']);

/** Hoisted — the defs have no per-render input once `rule` is dropped, and a
 *  fresh array each render would re-derive `numericCols` and remount the table. */
const RULE_POSITION_COLUMNS = historyColumns(() => null).filter(
  (c) => !OMIT_WHEN_SCOPED.has(c.key),
);
const RULE_POSITION_NUMERIC_COLS = numericColKeys(RULE_POSITION_COLUMNS);

/** Positions key by `id`, not by mint — a rule can re-enter the same token. */
const positionRowKey = (r: RulePositionRecord) => r.id;

/** The row's own entry/exit/target fills on its chart card — the same overlay the
 *  inspect modal draws, through the one `InspectTarget` adapter. */
const rulePositionRowOverlay = markerRowOverlay(inspectFromPosition);

/** Hold / PnL / exit-reason in the card header, so a card reads like its row. */
const rulePositionChartCardExtra = (r: RulePositionRecord) => (
  <PositionChartCardExtra facts={positionChartFactsFromRule(r)} />
);

const CLOSED_FIRST_SORT = { col: 'exit_time', dir: 'desc' } as const;

export const PortfolioRulePositions = memo(function PortfolioRulePositions({
  ruleId,
  ruleRow,
  range,
  mode,
  selectedPositionId,
  onSelectPosition,
  onClose,
}: {
  ruleId: string;
  /** The scoreboard row this panel drills into — its N / PnL are the figures the
   *  panel must reconcile with, so they are shown side by side in the header. */
  ruleRow: PortfolioRulePnl | null;
  range: PortfolioRange;
  mode: 'real' | 'paper';
  selectedPositionId: string | null;
  onSelectPosition: (positionId: string | null) => void;
  onClose: () => void;
}) {
  const { timezone } = useTimezone();
  // Frozen per mount, like the Console's cohort: a preset window whose `from`
  // slides on every render refetches the table continuously. The panel unmounts
  // on deselect, so it re-freezes whenever it is re-opened.
  const [nowMs] = useState(() => Date.now());

  // The table's own view state (search + column filters + page + sort).
  const [query, setQuery] = useState<TableQuery>(DEFAULT_POSITIONS_QUERY);

  const cohort = useMemo<HistoryCohort>(
    () => ({
      range,
      fromIso: presetStart(range, nowMs),
      toIso: null,
      ruleId,
      mode,
      // `lane` rather than `status: 'End'` — the lane is the aggregate's own
      // partition (entered AND ended), which is what the scoreboard row counts.
      status: null,
      exitReason: null,
      lane: 'closed',
      outcome: null,
      migrated: null,
      focus: null,
      seriesRange: range,
    }),
    [range, nowMs, ruleId, mode],
  );

  const body = useMemo(
    () =>
      historyTableBody({
        cohort,
        query,
        numericCols: RULE_POSITION_NUMERIC_COLS,
        timezone,
      }),
    [cohort, query, timezone],
  );

  const fetchPage = useCallback(
    (b: unknown, signal: AbortSignal) => fetchPortfolioPositionsPage(b as never, signal),
    [],
  );

  const { items, total, loading, error } = useServerTable<RulePositionRecord>(
    true,
    body,
    fetchPage,
  );

  const inspect = selectedPositionId
    ? (items.find((r) => r.id === selectedPositionId) ?? null)
    : null;

  const navKeys = useMemo(() => items.map((r) => r.id), [items]);
  usePositionArrowNav({
    enabled: !!inspect,
    keys: navKeys,
    selectedKey: selectedPositionId,
    onSelect: onSelectPosition,
  });

  const ruleName = ruleRow?.rule_name ?? `${ruleId.slice(0, 8)}…`;

  return (
    <div className="flex flex-col gap-2 rounded-lg border border-primary/25 bg-bg-panel p-3">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <h2 className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
          Closed trades
        </h2>
        <span className="text-xs font-semibold text-text">{ruleName}</span>
        <span className="text-[11px] text-text-dim">
          {loading && total === 0 ? 'loading…' : `${total} closed`} · {RANGE_LABEL[range]} ·{' '}
          {mode}
        </span>
        {ruleRow && (
          <span
            className={`tabular-nums text-xs font-semibold ${signedToneClass(ruleRow.realized_pnl_sol)}`}
            title="Realized PnL of this rule in this window — the rows below sum to it"
          >
            {formatSigned(ruleRow.realized_pnl_sol, 3)}◎
          </span>
        )}
        <button
          type="button"
          onClick={onClose}
          className="ml-auto inline-flex items-center gap-1 text-[11px] font-semibold text-text-dim transition hover:text-text"
          title="Clear the rule selection"
        >
          <CloseIcon className="h-3.5 w-3.5" />
          Close
        </button>
      </div>

      {error && <InlineAlert variant="error">Positions failed to load: {error}</InlineAlert>}

      <TokenTable
        columns={RULE_POSITION_COLUMNS}
        // This panel owns its full layout; an appended enrichment column would
        // offer a sort/filter key the backend whitelist rejects.
        existingKeys={ALL_TOKEN_INFO_KEYS}
        rows={items}
        rowKey={positionRowKey}
        serverSide
        serverTotal={total}
        onQueryChange={setQuery}
        loading={loading}
        searchable
        colFilters
        // Charts toggle, grid closed until asked: a card is a per-token trade
        // fetch, and selecting a rule must not fire 20 of them. The choice then
        // persists per `tableId`.
        charts
        useRowOverlay={rulePositionRowOverlay}
        renderChartCardExtra={rulePositionChartCardExtra}
        // Changing rule / window / mode is a new population — snap back to page 1.
        // The table's own search + filters are deliberately absent from the key,
        // or every keystroke would reset the table that produced it.
        resetKey={historyCohortKey(cohort, timezone)}
        defaultSort={CLOSED_FIRST_SORT}
        defaultPageSize={DEFAULT_POSITIONS_QUERY.pageSize}
        tableId="portfolio-rule-positions"
        emptyMessage="No closed trades for this rule in this window."
        selectedKey={selectedPositionId}
        onSelect={onSelectPosition}
      />

      {inspect && (
        <LazyLivePositionInspectModal
          position={inspect}
          rule={null}
          ruleName={ruleRow?.rule_name ?? null}
          onClose={() => onSelectPosition(null)}
        />
      )}
    </div>
  );
});
