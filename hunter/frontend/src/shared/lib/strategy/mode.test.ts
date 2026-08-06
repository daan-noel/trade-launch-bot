import { describe, expect, it } from 'vitest';
import { twMerge } from 'tailwind-merge';

import {
  DEFAULT_MODE_FILTER,
  isDefaultModeFilter,
  matchesModeFilter,
  modeCounts,
  parseModeFilter,
} from './mode';
import { modeRuleRowClass, ruleRowClass } from './types';

describe('mode filter', () => {
  it('parses the three scopes and falls back to `all` on junk', () => {
    expect(parseModeFilter('paper')).toBe('paper');
    expect(parseModeFilter('real')).toBe('real');
    expect(parseModeFilter('all')).toBe('all');
    // A bad/absent param must never blank the board.
    expect(parseModeFilter(null)).toBe(DEFAULT_MODE_FILTER);
    expect(parseModeFilter('REAL')).toBe(DEFAULT_MODE_FILTER);
    expect(parseModeFilter('')).toBe(DEFAULT_MODE_FILTER);
  });

  it('`all` admits both modes; a scope admits only its own', () => {
    expect(matchesModeFilter('paper', 'all')).toBe(true);
    expect(matchesModeFilter('real', 'all')).toBe(true);
    expect(matchesModeFilter('paper', 'paper')).toBe(true);
    expect(matchesModeFilter('real', 'paper')).toBe(false);
    expect(matchesModeFilter('paper', 'real')).toBe(false);
    expect(isDefaultModeFilter('all')).toBe(true);
    expect(isDefaultModeFilter('real')).toBe(false);
  });

  it('counts every mode, including the ones with no rules', () => {
    expect(modeCounts([])).toEqual({ paper: 0, real: 0 });
    expect(
      modeCounts([{ trade_mode: 'paper' }, { trade_mode: 'real' }, { trade_mode: 'paper' }]),
    ).toEqual({ paper: 2, real: 1 });
  });
});

describe('rule row paint', () => {
  const paper = { trade_mode: 'paper', is_enabled: true } as const;
  const real = { trade_mode: 'real', is_enabled: true } as const;

  it('gives the two modes visibly different rails', () => {
    expect(modeRuleRowClass(real)).not.toBe(modeRuleRowClass(paper));
    expect(modeRuleRowClass(real)).toContain('--color-warning');
    expect(modeRuleRowClass(paper)).toContain('--color-info');
  });

  /**
   * The load-bearing one. `DataTable` merges `rowClassName` LAST through
   * tailwind-merge, so a background **color** here would collapse the row's
   * selection (`bg-accent/16`) and pin (`bg-primary/4.5`) washes into nothing —
   * the mode rail would silently eat the selection highlight on every rule
   * board. A gradient is a different merge group, so both survive. If this
   * fails, the rail was rewritten as a `bg-<color>` utility.
   */
  it('survives tailwind-merge alongside the selection and pin washes', () => {
    for (const rule of [paper, real]) {
      const merged = twMerge('bg-primary/4.5', 'bg-accent/16', ruleRowClass(rule));
      expect(merged).toContain('bg-accent/16');
      expect(merged).toContain(modeRuleRowClass(rule));
    }
  });

  it('keeps the rail on a soft-archived rule and still dims it', () => {
    const archived = ruleRowClass({ trade_mode: 'real', is_enabled: false });
    expect(archived).toContain(modeRuleRowClass(real));
    expect(archived).toContain('opacity-40');
  });
});
