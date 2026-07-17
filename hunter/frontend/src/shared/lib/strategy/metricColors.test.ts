import { describe, expect, it } from 'vitest';
import { hashMetricHue, metricColorStyle } from './metricColors';

describe('metricColorStyle', () => {
  it('uses the registry hue and ignores side', () => {
    const a = metricColorStyle({ hue: 200, group: 'm_snapshot', metric: 'time', operator: '>' });
    const b = metricColorStyle({ hue: 200, group: 'm_snapshot', metric: 'time', operator: '>' });
    expect(a.hue).toBe(200);
    expect(a.border).toBe(b.border);
    expect(a.background).toBe(b.background);
  });

  it('applies a fixed shade per operator', () => {
    const gt = metricColorStyle({ hue: 200, group: 'm_snapshot', metric: 'time', operator: '>' });
    const eq = metricColorStyle({ hue: 200, group: 'm_snapshot', metric: 'time', operator: '=' });
    expect(gt.border).not.toBe(eq.border);
    expect(gt.hue).toBe(eq.hue);
  });

  it('falls back to a stable hash when hue is missing', () => {
    const a = metricColorStyle({ group: 'm_snapshot', metric: 'time', operator: '>' });
    const b = metricColorStyle({ group: 'm_snapshot', metric: 'time', operator: '>' });
    expect(a.hue).toBe(hashMetricHue('m_snapshot', 'time'));
    expect(a.border).toBe(b.border);
    expect(a.hue).not.toBe(hashMetricHue('m_snapshot', 'liquidity'));
  });
});
