import { describe, expect, it } from 'vitest';
import {
  DEFAULT_CREATION_WINDOW,
  creationWindowDraft,
  resolveCreationWindow,
  toCreationWindow,
} from './creationStats';

describe('toCreationWindow', () => {
  it('reads the legacy bare day count as the equivalent preset', () => {
    expect(toCreationWindow(30)).toEqual({ preset: '30', from: '', to: '' });
  });

  it('falls back for a day count that is not a preset, and for junk', () => {
    expect(toCreationWindow(42)).toEqual(DEFAULT_CREATION_WINDOW);
    expect(toCreationWindow(null)).toEqual(DEFAULT_CREATION_WINDOW);
    expect(toCreationWindow({ preset: 'nope' })).toEqual(DEFAULT_CREATION_WINDOW);
  });

  it('keeps a stored custom range, defaulting missing bounds to open', () => {
    expect(toCreationWindow({ preset: 'custom', from: '2026-08-01T00:00' })).toEqual({
      preset: 'custom',
      from: '2026-08-01T00:00',
      to: '',
    });
  });
});

describe('resolveCreationWindow', () => {
  it('opens Today at the zone midnight and leaves the upper bound open', () => {
    const { from, to } = resolveCreationWindow(
      { preset: 'today', from: '', to: '' },
      'Europe/Amsterdam',
    );
    // Amsterdam is UTC+1/+2, so civil midnight is the previous UTC evening.
    expect(from).toMatch(/T2[23]:00:00Z$/);
    expect(to).toBeUndefined();
  });

  it('closes Yesterday at the zone midnight, one civil day wide', () => {
    const { from, to, spanDays } = resolveCreationWindow(
      { preset: 'yesterday', from: '', to: '' },
      'UTC',
    );
    expect(from).toMatch(/T00:00:00Z$/);
    expect(to).toMatch(/T00:00:00Z$/);
    expect(spanDays).toBe(1);
  });

  it('spans exactly the look-back days for a rolling preset, with no upper bound', () => {
    const { from, to, spanDays } = resolveCreationWindow(
      { preset: '7', from: '', to: '' },
      'UTC',
    );
    expect(spanDays).toBe(7);
    expect(to).toBeUndefined();
    expect(Date.now() - Date.parse(from)).toBeGreaterThan(6.9 * 86_400_000);
  });

  it('converts custom bounds out of the display zone', () => {
    const { from, to, spanDays } = resolveCreationWindow(
      { preset: 'custom', from: '2026-08-01T00:00', to: '2026-08-03T00:00' },
      'UTC',
    );
    expect(from).toBe('2026-08-01T00:00:00Z');
    expect(to).toBe('2026-08-03T00:00:00Z');
    expect(spanDays).toBe(2);
  });

  it('falls back to the default look-back when a custom lower bound is open', () => {
    const { from, spanDays } = resolveCreationWindow(
      { preset: 'custom', from: '', to: '' },
      'UTC',
    );
    // `from` floors to the hour (stable cache key), so the span is 30d + <1h.
    expect(spanDays).toBeCloseTo(30, 1);
    expect(Date.parse(from)).toBeLessThan(Date.now());
  });

  it('falls back rather than inverting when the bounds are backwards', () => {
    const { spanDays } = resolveCreationWindow(
      { preset: 'custom', from: '2026-08-03T00:00', to: '2026-08-01T00:00' },
      'UTC',
    );
    expect(spanDays).toBe(30);
  });
});

describe('creationWindowDraft', () => {
  it('seeds a preset draft with the bounds that preset resolves to', () => {
    const draft = creationWindowDraft({ preset: 'yesterday', from: '', to: '' }, 'UTC');
    expect(draft.preset).toBe('yesterday');
    expect(draft.from).toMatch(/^\d{4}-\d{2}-\d{2}T00:00$/);
    expect(draft.to).toMatch(/^\d{4}-\d{2}-\d{2}T00:00$/);
  });

  it('passes a custom draft through verbatim', () => {
    const win = { preset: 'custom' as const, from: '2026-08-01T06:30', to: '' };
    expect(creationWindowDraft(win, 'UTC')).toEqual(win);
  });
});
