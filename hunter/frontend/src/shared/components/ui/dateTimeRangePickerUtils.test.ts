import { describe, expect, it } from 'vitest';
import {
  applyDayPick,
  buildMonthCells,
  defaultZoneBadge,
  formatCompact,
  formatYmdCompact,
  isoToPickerInput,
  isYmdOutOfBounds,
  joinDt,
  pickerInputToIso,
  normalizeTime,
  rangeDayRole,
  resolvePreviewBounds,
  splitDt,
  todayInZone,
} from './dateTimeRangePickerUtils';

describe('splitDt / joinDt', () => {
  it('round-trips wall-clock values', () => {
    expect(splitDt('2026-08-06T14:30')).toEqual({ date: '2026-08-06', time: '14:30' });
    expect(joinDt('2026-08-06', '14:30')).toBe('2026-08-06T14:30');
    expect(joinDt('2026-08-06', '')).toBe('2026-08-06T00:00');
    expect(joinDt('', '14:30')).toBe('');
  });

  it('formats compact trigger labels', () => {
    expect(formatCompact('2026-08-06T14:30')).toBe('08/06 14:30');
    expect(formatCompact('')).toBe('');
  });
});

describe('applyDayPick', () => {
  it('first click sets start and advances to end', () => {
    expect(
      applyDayPick({ from: '', to: '', picking: 'from' }, '2026-08-01', 'custom'),
    ).toEqual({
      preset: 'custom',
      from: '2026-08-01T00:00',
      to: '',
      picking: 'to',
    });
  });

  it('second click completes the range', () => {
    expect(
      applyDayPick(
        { from: '2026-08-01T00:00', to: '', picking: 'to' },
        '2026-08-05',
        'custom',
      ),
    ).toEqual({
      preset: 'custom',
      from: '2026-08-01T00:00',
      to: '2026-08-05T23:59',
      picking: 'from',
    });
  });

  it('click before start swaps bounds', () => {
    expect(
      applyDayPick(
        { from: '2026-08-05T00:00', to: '', picking: 'to' },
        '2026-08-01',
        'custom',
      ),
    ).toEqual({
      preset: 'custom',
      from: '2026-08-01T00:00',
      to: '2026-08-05T23:59',
      picking: 'from',
    });
  });

  it('click after a complete range restarts at start', () => {
    expect(
      applyDayPick(
        { from: '2026-08-01T00:00', to: '2026-08-05T23:59', picking: 'from' },
        '2026-08-10',
        'custom',
      ),
    ).toEqual({
      preset: 'custom',
      from: '2026-08-10T00:00',
      to: '',
      picking: 'to',
    });
  });
});

describe('hover preview', () => {
  it('previews a forward range while picking end', () => {
    expect(
      resolvePreviewBounds('2026-08-01', '', 'to', '2026-08-05'),
    ).toEqual({ lo: '2026-08-01', hi: '2026-08-05', previewing: true });
  });

  it('previews a backward hover by swapping', () => {
    expect(
      resolvePreviewBounds('2026-08-05', '', 'to', '2026-08-01'),
    ).toEqual({ lo: '2026-08-01', hi: '2026-08-05', previewing: true });
  });

  it('paints preview-end / middle roles (ordered L→R)', () => {
    expect(rangeDayRole('2026-08-01', '2026-08-01', '', 'to', '2026-08-05')).toBe(
      'start',
    );
    expect(rangeDayRole('2026-08-03', '2026-08-01', '', 'to', '2026-08-05')).toBe(
      'preview-middle',
    );
    expect(rangeDayRole('2026-08-05', '2026-08-01', '', 'to', '2026-08-05')).toBe(
      'preview-end',
    );
    // Hover before committed start — lo is the hover day.
    expect(rangeDayRole('2026-08-01', '2026-08-05', '', 'to', '2026-08-01')).toBe(
      'start',
    );
    expect(rangeDayRole('2026-08-05', '2026-08-05', '', 'to', '2026-08-01')).toBe(
      'preview-end',
    );
  });

  it('uses committed roles once both ends exist', () => {
    expect(
      rangeDayRole('2026-08-03', '2026-08-01', '2026-08-05', 'from', '2026-08-10'),
    ).toBe('middle');
    expect(
      rangeDayRole('2026-08-01', '2026-08-01', '2026-08-01', 'from', null),
    ).toBe('single');
  });
});

describe('normalizeTime / zone helpers', () => {
  it('strips seconds from native time inputs', () => {
    expect(normalizeTime('14:30:59')).toBe('14:30');
    expect(normalizeTime('14:30')).toBe('14:30');
    expect(normalizeTime('')).toBe('');
  });

  it('builds compact zone badges', () => {
    expect(defaultZoneBadge('UTC')).toBe('UTC');
    expect(defaultZoneBadge('America/New_York')).toBe('New York');
  });

  it('resolves civil today in UTC', () => {
    const { ymd, ym } = todayInZone('UTC');
    const n = new Date();
    expect(ymd).toBe(
      `${n.getUTCFullYear()}-${String(n.getUTCMonth() + 1).padStart(2, '0')}-${String(n.getUTCDate()).padStart(2, '0')}`,
    );
    expect(ym).toEqual({ y: n.getUTCFullYear(), m: n.getUTCMonth() });
  });

  it('formats and bounds single-day values', () => {
    expect(formatYmdCompact('2026-08-06')).toBe('08/06/2026');
    expect(isYmdOutOfBounds('2026-08-07', undefined, '2026-08-06')).toBe(true);
    expect(isYmdOutOfBounds('2026-08-06', undefined, '2026-08-06')).toBe(false);
    const cells = buildMonthCells(2026, 7); // August
    expect(cells.filter((c) => c.day != null)).toHaveLength(31);
  });
});

describe('isoToPickerInput / pickerInputToIso', () => {
  it('round-trips a UTC cohort bound through the picker wire value', () => {
    expect(isoToPickerInput('2026-08-06T14:30:00.000Z')).toBe('2026-08-06T14:30');
    expect(pickerInputToIso('2026-08-06T14:30')).toBe('2026-08-06T14:30:00.000Z');
  });

  it('maps an absent bound both ways', () => {
    expect(isoToPickerInput(null)).toBe('');
    expect(pickerInputToIso('')).toBeNull();
  });

  it('reads the wall-clock as UTC, never as the browser zone', () => {
    expect(pickerInputToIso('2026-01-01T00:00')).toBe('2026-01-01T00:00:00.000Z');
  });

  it('rejects an unparseable wall-clock instead of emitting Invalid Date', () => {
    expect(pickerInputToIso('not-a-date')).toBeNull();
  });
});
