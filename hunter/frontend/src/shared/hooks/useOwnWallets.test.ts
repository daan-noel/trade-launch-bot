import { describe, expect, it } from 'vitest';
import { classifyOptsForTag } from 'lib/flow/tapeClassify';
import { withOwnWallets } from './useOwnWallets';

describe('withOwnWallets', () => {
  it('adds our wallets to the lens exclusion', () => {
    const out = withOwnWallets(new Set(['trader']), new Set(['mine1', 'mine2']));
    expect([...(out ?? [])].sort()).toEqual(['mine1', 'mine2', 'trader']);
  });

  it('is the own set alone when nothing else is excluded, and null when both are empty', () => {
    const own = new Set(['mine1']);
    expect(withOwnWallets(null, own)).toBe(own);
    expect(withOwnWallets(null, new Set())).toBeNull();
  });

  it('reaches the classifier options, which skip excluded wallets', () => {
    const tag = { name: 'volume', match: { wallet: ['w'] }, sticky: false } as never;
    const opts = classifyOptsForTag(tag, null, withOwnWallets(null, new Set(['mine1'])));
    expect(opts?.excludeWallets?.has('mine1')).toBe(true);
  });
});
