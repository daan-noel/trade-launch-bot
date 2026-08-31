import { describe, expect, it } from 'vitest';
import { classifyOptsForTape, keysForTapeDraft } from './tapeClassify';

describe('classifyOptsForTape', () => {
  const keys = new Set([JSON.stringify(['A'])]);

  it('tagged defaults to contagion + creator seed', () => {
    const opts = classifyOptsForTape({
      list: 'tagged',
      keys,
      creatorWallet: 'creator',
    });
    expect(opts).toMatchObject({
      match: 'labels',
      contagion: true,
      creatorWallet: 'creator',
    });
  });

  it('dump and working turn contagion and creator seed off', () => {
    for (const list of ['dump', 'working'] as const) {
      const opts = classifyOptsForTape({
        list,
        keys,
        creatorWallet: 'creator',
      });
      expect(opts).toMatchObject({
        contagion: false,
        creatorWallet: null,
        match: list === 'working' ? 'grain' : 'labels',
      });
    }
  });

  it('empty dump keys with no contagion classify nothing', () => {
    expect(classifyOptsForTape({ list: 'dump', keys: null, creatorWallet: 'c' })).toBeNull();
  });

  it('empty tagged keys still classify via creator contagion', () => {
    const opts = classifyOptsForTape({ list: 'tagged', keys: null, creatorWallet: 'c' });
    expect(opts).not.toBeNull();
    expect(opts?.creatorWallet).toBe('c');
  });

  it('staging can suppress contagion on tagged', () => {
    const opts = classifyOptsForTape({
      list: 'tagged',
      keys,
      creatorWallet: 'c',
      contagion: false,
    });
    expect(opts?.contagion).toBe(false);
  });
});

describe('keysForTapeDraft', () => {
  it('uses label keys for tagged/dump and grain ids for working', () => {
    const rows = [{ labels: ['A', 'B'] }];
    expect([...keysForTapeDraft('tagged', rows, [])!]).toEqual([JSON.stringify(['A', 'B'])]);
    expect(keysForTapeDraft('working', rows, ['Pump.Fun|CU'])).toEqual(new Set(['Pump.Fun|CU']));
    expect(keysForTapeDraft('working', rows, [])).toBeNull();
  });
});
