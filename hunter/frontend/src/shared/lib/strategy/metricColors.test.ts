import { describe, expect, it } from 'vitest';
import { hashMetricHue, metricColorStyle, metricTone } from './metricColors';

describe('metricColorStyle', () => {
  it('uses the registry hue and ignores side', () => {
    const a = metricColorStyle({ hue: 200, group: 'm_state', metric: 'time', operator: '>' });
    const b = metricColorStyle({ hue: 200, group: 'm_state', metric: 'time', operator: '>' });
    expect(a.hue).toBe(200);
    expect(a.border).toBe(b.border);
    expect(a.background).toBe(b.background);
  });

  it('applies a fixed shade per operator', () => {
    const gt = metricColorStyle({ hue: 200, group: 'm_state', metric: 'time', operator: '>' });
    const eq = metricColorStyle({ hue: 200, group: 'm_state', metric: 'time', operator: '=' });
    expect(gt.border).not.toBe(eq.border);
    expect(gt.hue).toBe(eq.hue);
  });

  it('keeps a family in one hue and steps lightness for a neighbor', () => {
    const a = metricTone(170);
    const b = metricTone(174);
    expect(a.hue).toBe(170);
    expect(b.hue).toBe(174);
    expect(a.color).not.toBe(b.color);
    expect(metricTone(355).hue).not.toBe(a.hue);
  });

  it('keeps the wash light and the text strong', () => {
    const tone = metricTone(200);
    const sat = Number(tone.color.match(/,\s*([\d.]+)%/)?.[1]);
    const light = Number(tone.color.match(/([\d.]+)%\)/)?.[1]);
    expect(sat).toBeGreaterThanOrEqual(68);
    expect(light).toBeGreaterThanOrEqual(74);
    expect(tone.background).toContain('0.1');
  });

  it('falls back to a stable hash when hue is missing', () => {
    const a = metricColorStyle({ group: 'm_state', metric: 'time', operator: '>' });
    const b = metricColorStyle({ group: 'm_state', metric: 'time', operator: '>' });
    expect(a.hue).toBe(hashMetricHue('m_state', 'time'));
    expect(a.border).toBe(b.border);
    expect(a.hue).not.toBe(hashMetricHue('m_state', 'liquidity'));
  });
});
