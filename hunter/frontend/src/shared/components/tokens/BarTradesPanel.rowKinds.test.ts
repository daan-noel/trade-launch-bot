import { describe, expect, it } from 'vitest';
import { buildEntryExitMap } from './BarTradesPanel';
import type { ChartEventMarker } from 'components/token-price-chart/types';

const at = '2026-09-11T00:00:00Z';

describe('buildEntryExitMap', () => {
  it('tints fills by side and the trigger print as a signal', () => {
    const markers: ChartEventMarker[] = [
      { kind: 'entry', role: 'signal', time: at, priceInSol: 1, txSignature: 'trig' },
      { kind: 'entry', role: 'fill', time: at, priceInSol: 1, txSignature: 'buy' },
      { kind: 'exit', time: at, priceInSol: 1, txSignature: 'sell' },
    ];
    const m = buildEntryExitMap(markers);
    expect(m.get('trig')).toBe('signal');
    expect(m.get('buy')).toBe('entry');
    expect(m.get('sell')).toBe('exit');
  });

  it('lets a fill on the trigger print outrank the signal, in either order', () => {
    const signal: ChartEventMarker = {
      kind: 'entry',
      role: 'signal',
      time: at,
      priceInSol: 1,
      txSignature: 'same',
    };
    const fill: ChartEventMarker = { kind: 'entry', time: at, priceInSol: 1, txSignature: 'same' };
    expect(buildEntryExitMap([signal, fill]).get('same')).toBe('entry');
    expect(buildEntryExitMap([fill, signal]).get('same')).toBe('entry');
  });

  it('skips markers with no signature', () => {
    const m = buildEntryExitMap([{ kind: 'entry', time: at, priceInSol: 1, txSignature: '' }]);
    expect(m.size).toBe(0);
  });
});
