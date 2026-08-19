/**
 * The look-back control every creation-stats surface uses: civil-day shortcuts
 * (Today / Yesterday), the rolling look-backs, and an absolute date+time range
 * behind `Custom`.
 *
 * One component so the page, the grouped section and any future creation-stats
 * surface offer the SAME window vocabulary — the shortcut set, the timezone the
 * civil days resolve in, and the wall-clock -> RFC3339 lowering all live in
 * `creationStats.ts` next to it.
 */

import { useMemo } from 'react';
import { DateTimeRangePicker } from 'components/ui/DateTimeRangePicker';
import {
  CREATION_RANGE_PRESETS,
  creationWindowDraft,
  type CreationRangePreset,
  type CreationWindow,
} from './creationStats';

export interface CreationWindowPickerProps {
  value: CreationWindow;
  onChange: (next: CreationWindow) => void;
  /** Display timezone — civil-day presets and the custom bounds resolve in it. */
  timezone: string;
  size?: 'sm' | 'md';
  'aria-label'?: string;
  className?: string;
  disabled?: boolean;
}

export function CreationWindowPicker({
  value,
  onChange,
  timezone,
  size = 'sm',
  'aria-label': ariaLabel = 'Creation window',
  className,
  disabled,
}: CreationWindowPickerProps) {
  // Preset bounds are passed through too, so the popover draft opens on the
  // window that is actually applied instead of an empty calendar.
  const draft = useMemo(() => creationWindowDraft(value, timezone), [value, timezone]);

  return (
    <DateTimeRangePicker<CreationRangePreset>
      aria-label={ariaLabel}
      size={size}
      className={className}
      disabled={disabled}
      timeZone={timezone}
      emptyLabel="Look-back"
      presets={CREATION_RANGE_PRESETS}
      customPreset="custom"
      value={draft}
      onChange={({ preset, from, to }) =>
        onChange(
          preset === 'custom'
            ? { preset: 'custom', from, to }
            : // A shortcut owns its own bounds — drop the draft's so a later
              // timezone change re-resolves it instead of freezing old instants.
              { preset, from: '', to: '' },
        )
      }
    />
  );
}
