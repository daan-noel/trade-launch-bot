import { describe, expect, it } from 'vitest';

import { FP_CONFIG_LISTS } from './FingerprintParamsSummary';
import type { Fingerprint } from 'lib/strategy/types';

const fp = (tags: Record<string, unknown>): Fingerprint => ({
  id: 'x',
  name: 'x',
  wildcard: true,
  criteria: {},
  tags,
  created_at: '',
  updated_at: '',
});

describe('the tags list', () => {
  const spec = FP_CONFIG_LISTS.find((s) => s.key === 'tags')!;

  it('keys identity on contents, not key order', () => {
    const a = fp({ volume: { match: { creator: true, program: ['A'] }, sticky: true } });
    const b = fp({ volume: { sticky: true, match: { program: ['A'], creator: true } } });
    const c = fp({ volume: { match: { creator: true, program: ['B'] }, sticky: true } });
    expect(spec.identity(a)).toBe(spec.identity(b));
    expect(spec.identity(a)).not.toBe(spec.identity(c));
  });

  it('makes names and contents searchable', () => {
    const t = spec.searchText(fp({ targets: { match: { wallet: ['7xKXabc'] } } }));
    expect(t).toContain('@targets');
    expect(t).toContain('7xKXabc');
    expect(spec.count(fp({ a: { match: { creator: true } }, b: { match: { creator: true } } }))).toBe(2);
  });
});
