/**
 * The single cohort driver for the Console History section — date range · rule ·
 * mode · status · exit reason. Everything below it (charts deck AND the paged
 * table) reads the same cohort, so the charts can never describe a different
 * population than the rows underneath them.
 */

import { memo, useMemo } from 'react';
import { Badge } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { Input } from 'components/ui/Input';
import { SearchableSelect } from 'components/ui/SearchableSelect';
import { Select } from 'components/ui/Select';
import { useGetStrategyRulesQuery } from 'store/sharedEndpoints';
import type { HistoryRange } from 'lib/strategy/nav';
import type { HistoryCohortApi, HistoryMode } from '@live/pages/console/historyCohort';

const RANGES: { key: HistoryRange; label: string }[] = [
  { key: 'today', label: 'Today' },
  { key: '7d', label: '7d' },
  { key: '30d', label: '30d' },
  { key: 'all', label: 'All' },
  { key: 'custom', label: 'Custom' },
];

/** Terminal + open statuses a reviewer actually filters on. */
const STATUSES = [
  { value: 'End', label: 'Closed (End)' },
  { value: 'EntryFailed', label: 'Entry failed' },
  { value: 'Holding', label: 'Holding' },
  { value: 'ExitStuck', label: 'Exit stuck' },
  { value: 'ExitPending', label: 'Exit pending' },
  { value: 'ExitUnconfirmed', label: 'Exit unconfirmed' },
  { value: 'BuySubmitted', label: 'Buy submitted' },
];

/** Exit reasons the engine writes (the `Metrics(…)` family filters by substring). */
const EXIT_REASONS = [
  'TakeProfit',
  'StopLoss',
  'Trailing',
  'Stall',
  'Time',
  'Liquidity',
  'Dead',
  'Manual',
  'NextKill',
];

/** `datetime-local` wants a `YYYY-MM-DDTHH:MM` wall-clock; the cohort stores UTC ISO. */
function isoToLocalInput(iso: string | null): string {
  if (!iso) return '';
  return iso.slice(0, 16);
}
function localInputToIso(v: string): string | null {
  if (!v) return null;
  const d = new Date(`${v}:00Z`);
  return Number.isNaN(d.getTime()) ? null : d.toISOString();
}

export const HistoryFilterBar = memo(function HistoryFilterBar({
  cohort,
  closedCount,
  entryFailed,
}: {
  cohort: HistoryCohortApi;
  /** Closes in the cohort (from the series) — the honest "what am I looking at". */
  closedCount: number | null;
  entryFailed: number | null;
}) {
  const { data: rules = [] } = useGetStrategyRulesQuery();
  const ruleOptions = useMemo(
    () =>
      [...rules]
        .map((r) => ({ value: r.id, label: r.rule_name || r.id.slice(0, 8), data: r }))
        .sort((a, b) => a.label.localeCompare(b.label)),
    [rules],
  );

  return (
    <div className="flex flex-col gap-2 rounded-lg border border-white/6 bg-bg-panel p-2.5">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        <div className="flex items-center gap-1">
          {RANGES.map((r) => (
            <Button
              key={r.key}
              size="sm"
              variant={cohort.range === r.key ? 'primary' : 'subtle'}
              onClick={() => cohort.set({ range: r.key })}
            >
              {r.label}
            </Button>
          ))}
        </div>

        {cohort.range === 'custom' && (
          <div className="flex items-center gap-1.5">
            <Input
              type="datetime-local"
              fieldSize="sm"
              value={isoToLocalInput(cohort.fromIso)}
              onChange={(e) => cohort.set({ fromIso: localInputToIso(e.target.value) })}
              title="Window start (UTC)"
            />
            <span className="text-[10px] text-text-dim">→</span>
            <Input
              type="datetime-local"
              fieldSize="sm"
              value={isoToLocalInput(cohort.toIso)}
              onChange={(e) => cohort.set({ toIso: localInputToIso(e.target.value) })}
              title="Window end (UTC, exclusive)"
            />
          </div>
        )}

        <div className="min-w-[190px]">
          <SearchableSelect
            options={ruleOptions}
            value={cohort.ruleId}
            onChange={(v) => cohort.set({ ruleId: v || null })}
            emptyOptionLabel="All rules"
            placeholder="All rules"
            noResultsLabel="No rule matches"
          />
        </div>

        <Select
          value={cohort.mode}
          onChange={(e) => cohort.set({ mode: e.target.value as HistoryMode })}
          title="Execution mode"
        >
          <option value="real">Real</option>
          <option value="paper">Paper</option>
          <option value="all">Real + paper</option>
        </Select>

        <Select
          value={cohort.status ?? ''}
          onChange={(e) => cohort.set({ status: e.target.value || null })}
          title="Position status"
        >
          <option value="">Any status</option>
          {STATUSES.map((s) => (
            <option key={s.value} value={s.value}>
              {s.label}
            </option>
          ))}
        </Select>

        <Select
          value={cohort.exitReason ?? ''}
          onChange={(e) => cohort.set({ exitReason: e.target.value || null })}
          title="Exit reason"
        >
          <option value="">Any exit</option>
          {EXIT_REASONS.map((r) => (
            <option key={r} value={r}>
              {r}
            </option>
          ))}
        </Select>

        {cohort.active && (
          <Button size="sm" variant="ghost" onClick={cohort.reset}>
            Clear
          </Button>
        )}
      </div>

      <div className="flex flex-wrap items-center gap-2 text-[11px] text-text-dim">
        <span>
          Cohort:{' '}
          <span className="text-text">
            {closedCount != null ? `${closedCount} closed` : '…'}
          </span>
          {entryFailed != null && entryFailed > 0 && (
            <>
              {' · '}
              <span title="Buys that never filled — no SOL deployed, excluded from PnL">
                {entryFailed} entry-failed
              </span>
            </>
          )}
        </span>
        {cohort.mode === 'paper' && <Badge variant="neutral">paper</Badge>}
        {cohort.mode === 'all' && <Badge variant="neutral">real + paper</Badge>}
      </div>
    </div>
  );
});
