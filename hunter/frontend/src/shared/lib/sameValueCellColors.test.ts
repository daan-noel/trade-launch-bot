import { describe, expect, it } from 'vitest';
import { computeSameValueCellClasses } from './sameValueCellColors';

describe('computeSameValueCellClasses', () => {
  const rows = [
    { id: 'a', cu: 100, buy: 1 },
    { id: 'b', cu: 100, buy: 2 },
    { id: 'c', cu: 200, buy: 1 },
    { id: 'd', cu: null as number | null, buy: 3 },
  ];

  it('tints values shared by ≥2 rows', () => {
    const map = computeSameValueCellClasses(rows, (r) => r.id, [
      { key: 'cu', valueOf: (r) => (r.cu == null ? null : String(r.cu)) },
      { key: 'buy', valueOf: (r) => String(r.buy) },
    ]);
    expect(map.get('a\0cu')).toBe(map.get('b\0cu'));
    expect(map.get('a\0cu')).toBeTruthy();
    expect(map.has('c\0cu')).toBe(false); // unique
    expect(map.get('a\0buy')).toBe(map.get('c\0buy'));
    expect(map.has('b\0buy')).toBe(false);
  });

  it('skips null / unset values', () => {
    const map = computeSameValueCellClasses(rows, (r) => r.id, [
      { key: 'cu', valueOf: (r) => (r.cu == null ? null : String(r.cu)) },
    ]);
    expect(map.has('d\0cu')).toBe(false);
  });

  it('assigns distinct colors to distinct shared values', () => {
    const many = [
      { id: '1', v: 'x' },
      { id: '2', v: 'x' },
      { id: '3', v: 'y' },
      { id: '4', v: 'y' },
    ];
    const map = computeSameValueCellClasses(many, (r) => r.id, [
      { key: 'v', valueOf: (r) => r.v },
    ]);
    expect(map.get('1\0v')).not.toBe(map.get('3\0v'));
  });
});
