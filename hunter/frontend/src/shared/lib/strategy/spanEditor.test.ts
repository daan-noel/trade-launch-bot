import { describe, expect, it } from 'vitest';

import registryRaw from '../../../../../engine/fixtures/registry.json?raw';
import { findMetric, type StrategyRegistry } from './registry';
import { applySpanSpell, spanReading, spanSpells } from './spanEditor';

const reg = JSON.parse(registryRaw) as StrategyRegistry;

describe('span spellings', () => {
  it('offers life and the four window forms for a life-or-window metric', () => {
    const spec = findMetric(reg, 'm_flow.buy_sol')!;
    expect(spanSpells(reg, spec).span.map((s) => s.face)).toEqual(['(empty)', '10s', '20sl', '5p', '10s@2']);
    expect(spanSpells(reg, spec).slice).toEqual([]);
  });

  it('offers age60s and age0s for a since-age metric', () => {
    const spec = findMetric(reg, 'm_crowd.buyer_count')!;
    expect(spanSpells(reg, spec).span.map((s) => s.face)).toEqual(['age60s', 'age0s']);
  });

  it('offers a slice in each unit beside the window', () => {
    const spec = findMetric(reg, 'm_flow.slice_sol_share_pct')!;
    const spells = spanSpells(reg, spec);
    expect(spells.span.map((s) => s.face)).toEqual(['10s', '20sl', '5p', '10s@2']);
    expect(spells.slice.map((s) => s.face)).toEqual(['2s', '4sl', '2p']);
  });

  it('reads the current text in the same words as a sentence', () => {
    const buy = findMetric(reg, 'm_flow.buy_sol')!;
    expect(spanReading(buy, {}).text).toBe("over the coin's life");
    expect(spanReading(buy, { span: '10s@2' }).text).toBe('in the 10 s ending 2 s ago');
    expect(spanReading(buy, { span: '1sl' }).text).toBe('in this slot');
    expect(spanReading(buy, { span: '1p' }).text).toBe('on this print');
    expect(spanReading(buy, { span: '10x' }).bad).toBe(true);

    const slice = findMetric(reg, 'm_flow.slice_sol_share_pct')!;
    expect(spanReading(slice, { span: '30s', slice: '2s' }).text).toBe('in the last 30 s, of which the last 2 s');
    expect(spanReading(slice, { span: '30s', slice: '4sl' }).bad).toBe(true);

    const age = findMetric(reg, 'm_crowd.buyer_count')!;
    expect(spanReading(age, { span: 'age0s' }).text).toBe('since creation');
    expect(spanReading(age, { span: 'age60s' }).text).toBe('since age 60 s');
  });

  it('writes a slice in the span unit and keeps a lagged span aligned', () => {
    const spec = findMetric(reg, 'm_flow.slice_sol_share_pct')!;
    const slot = spanSpells(reg, spec).slice[1]!;
    expect(applySpanSpell(spec, { metric: spec.path, span: '30s@2', slice: '2s' }, slot)).toEqual({
      span: '30sl@2',
      slice: '4sl',
    });
    const secs = spanSpells(reg, spec).span[0]!;
    expect(applySpanSpell(spec, { metric: spec.path, span: '30sl', slice: '4sl' }, secs)).toEqual({
      span: '10s',
      slice: '2s',
    });
  });
});
