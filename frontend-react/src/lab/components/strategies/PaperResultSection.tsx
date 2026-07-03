import { useCallback, useState, type ReactNode } from 'react';
import { DataTable } from 'components/table/DataTable';
import { Badge, type BadgeVariant } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { SimSummaryCard } from 'components/tpsl1/SimSummaryCard';
import { useTimezone } from 'context/TimezoneContext';
import { formatIso } from 'utils/date';
import { cn } from 'lib/cn';
import type { usePriceDisplay } from 'hooks/usePriceDisplay';
import type { ColumnDef, TableQuery } from 'components/table/types';
import type {
  PaperResultResponse,
  PositionsSummary,
  RulePositionRecord,
} from 'types';

/** Inline section heading (marker bar + title + optional count badge + subtitle +
 *  actions) — mirrors the per-page `SectionHeading`, kept local so this shared
 *  component has no cross-page dependency. */
function SectionHeading({
  title,
  count,
  marker = 'bg-primary',
  badge = 'primary',
  subtitle,
  action,
}: {
  title: string;
  count?: number;
  marker?: string;
  badge?: BadgeVariant;
  subtitle?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="mb-3.5 flex items-center gap-2.5">
      <span className={cn('h-4 w-1 rounded-full', marker)} />
      <h3 className="text-sm font-bold text-text">{title}</h3>
      {count != null && (
        <Badge variant={badge} size="sm" className="font-mono font-normal">
          {count}
        </Badge>
      )}
      {subtitle && <span className="truncate font-mono text-[11px] text-text-dim">{subtitle}</span>}
      {action && (
        <>
          <span className="flex-1" />
          {action}
        </>
      )}
    </div>
  );
}

/**
 * The latest paper-test run for a rule: run-status header + Clear control, the
 * shared summary card, and the per-token positions table.
 *
 * The token table is **server-side** — it reuses the exact `useRulePositions`
 * machinery the Positions section uses (paged/sorted/filtered/searched in
 * Postgres, joined to token enrichment). The parent owns that hook instance
 * (scoped to this run's rule) and passes the current page + whole-run summary
 * down here; this component stays presentational apart from its own Clear-confirm
 * state. `run` metadata still comes from the `paper-result` payload (cheap; the
 * heavy token list no longer rides that request).
 */
export function PaperResultSection({
  data,
  positionColumns,
  positions,
  positionsTotal,
  positionsSummary,
  positionsLoading,
  price,
  tableId,
  resetKey,
  selectedKey,
  onSelectPosition,
  onQueryChange,
  onClose,
  onClear,
  clearing,
  canClear,
}: {
  data: PaperResultResponse;
  positionColumns: ColumnDef<RulePositionRecord>[];
  /** Current page of the run's positions (server-side window). */
  positions: RulePositionRecord[];
  /** Run-wide total for the pager (from `X-Total-Count`). */
  positionsTotal: number;
  /** Run-wide aggregates for the summary card (null until loaded). */
  positionsSummary: PositionsSummary | null;
  positionsLoading: boolean;
  price: ReturnType<typeof usePriceDisplay>;
  tableId: string;
  /** Snap the table back to page 1 when the open paper rule changes. */
  resetKey: string;
  selectedKey: string | null;
  onSelectPosition: (key: string | null) => void;
  onQueryChange: (q: TableQuery) => void;
  onClose: () => void;
  onClear: () => void;
  clearing: boolean;
  canClear: boolean;
}) {
  const { timezone } = useTimezone();
  const { run } = data;
  // Inline confirm for the destructive Clear (mirrors the row-delete pattern).
  const [confirmClear, setConfirmClear] = useState(false);

  const handleSelect = useCallback(
    (key: string | null) => onSelectPosition(key),
    [onSelectPosition],
  );

  if (!run) {
    return (
      <section>
        <SectionHeading
          title="Paper Test"
          marker="bg-info"
          badge="info"
          subtitle={data.rule_name}
          action={
            <button
              type="button"
              onClick={onClose}
              className="text-text-dim transition hover:text-text"
            >
              ✕
            </button>
          }
        />
        <p className="text-text-dim">
          This rule hasn&apos;t been run in paper mode yet. Activate it to start a paper test.
        </p>
      </section>
    );
  }

  const statusVariant: BadgeVariant =
    run.status === 'Finished' ? 'primary' : run.status === 'Stopped' ? 'neutral' : 'info';

  return (
    <>
      <div className="mb-4 flex flex-wrap items-center gap-x-4 gap-y-2 text-[11px] text-text-dim">
        <Badge variant={statusVariant} size="sm" pill className="uppercase">
          {run.status === 'Running' ? '● Running' : run.status}
        </Badge>
        <span className="font-mono">Run #{run.run_seq}</span>
        <span>
          Cap:{' '}
          <span className="font-mono text-text">
            {run.max_total_tokens != null ? run.max_total_tokens : '∞'}
          </span>
        </span>
        <span>
          Started <span className="font-mono text-text">{formatIso(run.started_at, timezone)}</span>
        </span>
        {run.finished_at && (
          <span>
            Ended <span className="font-mono text-text">{formatIso(run.finished_at, timezone)}</span>
          </span>
        )}
        <span className="flex-1" />
        {confirmClear ? (
          <span className="flex items-center gap-1">
            <span className="font-semibold text-red">Clear results?</span>
            <Button variant="danger" size="xs" disabled={clearing} onClick={onClear}>
              {clearing ? 'Clearing…' : 'Yes'}
            </Button>
            <Button
              variant="ghost"
              size="xs"
              disabled={clearing}
              onClick={() => setConfirmClear(false)}
            >
              No
            </Button>
          </span>
        ) : (
          <Button
            variant="ghost"
            size="xs"
            disabled={!canClear}
            onClick={() => setConfirmClear(true)}
            title={
              canClear
                ? 'Clear results — delete this rule’s paper run history'
                : 'Stop the rule before clearing its results'
            }
            className="text-red"
          >
            🗑 Clear results
          </Button>
        )}
      </div>

      {/* Summary is server-computed over the whole run (not the visible page). */}
      {positionsSummary && positionsSummary.tokens > 0 && (
        <SimSummaryCard
          title="Paper Test Results"
          ruleName={data.rule_name}
          summary={positionsSummary}
          price={price}
          onClose={onClose}
        />
      )}

      <section>
        <SectionHeading
          title="Paper Positions"
          count={positionsTotal}
          subtitle={data.rule_name}
        />
        <DataTable
          columns={positionColumns}
          rows={positions}
          rowKey={keyById}
          selectedKey={selectedKey}
          onSelect={handleSelect}
          serverSide
          serverTotal={positionsTotal}
          onQueryChange={onQueryChange}
          loading={positionsLoading}
          resetKey={resetKey}
          defaultPageSize={20}
          pageSizeOptions={[20, 50, 100]}
          colFilters
          colToggle
          tableId={tableId}
          emptyMessage="No positions recorded for this run yet."
        />
      </section>
    </>
  );
}

const keyById = (r: { id: string }) => r.id;
