/**
 * Slim All-Tokens cohort bar — Created range + Dead / Migrated. Lives on the
 * Tokens *page*, not inside DataTable (same layering as HistoryFilterBar).
 *
 * Created bounds are wall-clock in the project `timezone`; TokensPage converts
 * via `datetimeLocalToUtcWallClock` at the query boundary.
 */
import { memo, useMemo } from 'react';
import { Button } from 'components/ui/Button';
import { DateTimeRangePicker } from 'components/ui/DateTimeRangePicker';
import { utcIsoToDatetimeLocal } from 'utils/date';
import { ToggleGroup } from 'components/ui/ToggleGroup';
import {
  activeQuickFilterCount,
  clearedQuickFilters,
  rollingOneDayFromUtc,
  type CreatedPreset,
  type TokensQuickFilters,
  type TriState,
} from './tokensQuickFilters';

const CREATED_PRESETS: { value: CreatedPreset; label: string; description?: string }[] = [
  { value: '1d', label: '1 day' },
  { value: 'all', label: 'Any time' },
  { value: 'custom', label: 'Custom', description: 'exact date + time bounds' },
];

const TRI_OPTIONS: { value: TriState; label: string }[] = [
  { value: '', label: 'All' },
  { value: 'yes', label: 'Yes' },
  { value: 'no', label: 'No' },
];

export const TokensFilterBar = memo(function TokensFilterBar({
  filters,
  onChange,
  timezone,
}: {
  filters: TokensQuickFilters;
  onChange: (next: TokensQuickFilters) => void;
  /** Project IANA zone — picker wall-clock + Today ring. */
  timezone: string;
}) {
  const count = activeQuickFilterCount(filters);
  const createdValue = useMemo(() => {
    if (filters.created_preset === '1d') {
      return {
        preset: '1d' as const,
        from: utcIsoToDatetimeLocal(`${rollingOneDayFromUtc()}Z`, timezone),
        to: '',
      };
    }
    if (filters.created_preset === 'all') {
      return { preset: 'all' as const, from: '', to: '' };
    }
    return {
      preset: 'custom' as const,
      from: filters.created_from,
      to: filters.created_to,
    };
  }, [filters.created_preset, filters.created_from, filters.created_to, timezone]);

  return (
    <div className="mb-2 flex flex-wrap items-end gap-x-4 gap-y-2 rounded-lg border border-white/8 bg-white/2 px-3 py-2">
      <div className="flex flex-col gap-1">
        <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim/80">
          Created
        </span>
        <DateTimeRangePicker<CreatedPreset>
          aria-label="Created"
          timeZone={timezone}
          emptyLabel="Any time"
          presets={CREATED_PRESETS}
          customPreset="custom"
          value={createdValue}
          onChange={({ preset, from, to }) =>
            onChange(
              preset === 'custom'
                ? { ...filters, created_preset: 'custom', created_from: from, created_to: to }
                : { ...filters, created_preset: preset, created_from: '', created_to: '' },
            )
          }
        />
      </div>

      <div className="flex flex-col gap-1">
        <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim/80">Dead</span>
        <ToggleGroup
          aria-label="Dead"
          size="sm"
          tone="neutral"
          options={TRI_OPTIONS}
          value={filters.dead}
          onChange={(dead) => onChange({ ...filters, dead })}
        />
      </div>

      <div className="flex flex-col gap-1">
        <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim/80">Migrated</span>
        <ToggleGroup
          aria-label="Migrated"
          size="sm"
          tone="neutral"
          options={TRI_OPTIONS}
          value={filters.migrated}
          onChange={(migrated) => onChange({ ...filters, migrated })}
        />
      </div>

      <span className="flex-1" />

      <Button
        type="button"
        variant="ghost"
        size="xs"
        disabled={count === 0}
        onClick={() => onChange(clearedQuickFilters())}
        className="self-end"
      >
        Clear{count > 0 ? ` (${count})` : ''}
      </Button>
    </div>
  );
});
