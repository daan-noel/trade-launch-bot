import { describe, expect, it } from 'vitest';
import {
  classifyOptsForTag,
  defaultTagName,
  flowTagOf,
  shapeTag,
  tagLabel,
  withDraftMatch,
} from './tapeClassify';

describe('defaultTagName', () => {
  it('prefers volume, else the first tag, else volume', () => {
    expect(defaultTagName({ dump: {}, volume: {} })).toBe('volume');
    expect(defaultTagName({ dump: {}, crew: {} })).toBe('dump');
    expect(defaultTagName(null)).toBe('volume');
  });
});

describe('classifyOptsForTag', () => {
  it('is null for a missing tag or one with no matcher', () => {
    expect(classifyOptsForTag(null)).toBeNull();
    expect(classifyOptsForTag(shapeTag('v', []))).toBeNull();
  });

  it('carries the tag, the creator and the exclusions', () => {
    const tag = flowTagOf({ volume: { match: { creator: true }, sticky: true } }, 'volume')!;
    const opts = classifyOptsForTag(tag, 'dev', new Set(['me']));
    expect(opts?.tag.sticky).toBe(true);
    expect(opts?.creatorWallet).toBe('dev');
    expect(opts?.excludeWallets?.has('me')).toBe(true);
  });
});

describe('withDraftMatch', () => {
  it('replaces one matcher and keeps the rest; an empty list drops it', () => {
    const tag = flowTagOf({ v: { match: { ix_shape: [['a']], creator: true }, side: 'sell' } }, 'v')!;
    const draft = withDraftMatch(tag, { ix_shape: [{ labels: ['b'] }] });
    expect(draft.match).toEqual({ ix_shape: [{ labels: ['b'] }], creator: true });
    expect(draft.side).toBe('sell');
    expect(withDraftMatch(tag, { ix_shape: [] }).match).toEqual({ creator: true });
  });
});

describe('tagLabel', () => {
  it('writes both halves', () => {
    expect(tagLabel('volume')).toBe('@volume');
    expect(tagLabel('volume', true)).toBe('@!volume');
  });
});
