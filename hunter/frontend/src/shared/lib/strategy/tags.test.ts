import { describe, expect, it } from 'vitest';

import {
  cycleTag,
  EMPTY_TAG_FILTER,
  isEmptyTagFilter,
  matchesTagFilter,
  parseTagFilter,
  serializeTagFilter,
  tagChipState,
  tagCounts,
  tagNamespace,
  type TagFilterState,
} from './tags';

const rule = (...tags: string[]) => ({ tags });

describe('tag filter semantics', () => {
  it('passes everything when empty', () => {
    expect(matchesTagFilter(['fam:scalper'], EMPTY_TAG_FILTER)).toBe(true);
    expect(matchesTagFilter([], EMPTY_TAG_FILTER)).toBe(true);
    expect(matchesTagFilter(undefined, EMPTY_TAG_FILTER)).toBe(true);
  });

  it('ORs include chips', () => {
    const f: TagFilterState = { include: ['fam:scalper', 'fam:ignition'], exclude: [] };
    expect(matchesTagFilter(['fam:ignition'], f)).toBe(true);
    expect(matchesTagFilter(['fam:scalper', 'risk:high'], f)).toBe(true);
    expect(matchesTagFilter(['fam:swing'], f)).toBe(false);
    // Nothing to match ⇒ an untagged rule is hidden by an active include.
    expect(matchesTagFilter([], f)).toBe(false);
  });

  it('ANDs exclude chips and lets exclude win over include', () => {
    const f: TagFilterState = { include: [], exclude: ['stage:experiment'] };
    expect(matchesTagFilter(['fam:scalper'], f)).toBe(true);
    // Untagged rules survive a pure-exclude filter.
    expect(matchesTagFilter([], f)).toBe(true);
    expect(matchesTagFilter(['fam:scalper', 'stage:experiment'], f)).toBe(false);

    const both: TagFilterState = { include: ['fam:scalper'], exclude: ['stage:experiment'] };
    expect(matchesTagFilter(['fam:scalper', 'stage:experiment'], both)).toBe(false);
  });
});

describe('chip cycling', () => {
  it('goes off → include → exclude → off', () => {
    let f = EMPTY_TAG_FILTER;
    expect(tagChipState(f, 'a')).toBe('off');
    f = cycleTag(f, 'a');
    expect(tagChipState(f, 'a')).toBe('include');
    f = cycleTag(f, 'a');
    expect(tagChipState(f, 'a')).toBe('exclude');
    expect(f.include).toEqual([]);
    f = cycleTag(f, 'a');
    expect(tagChipState(f, 'a')).toBe('off');
    expect(isEmptyTagFilter(f)).toBe(true);
  });

  it('leaves other chips alone', () => {
    const f = cycleTag(cycleTag(EMPTY_TAG_FILTER, 'a'), 'b');
    expect(f.include).toEqual(['a', 'b']);
    expect(cycleTag(f, 'a')).toEqual({ include: ['b'], exclude: ['a'] });
  });
});

describe('catalog', () => {
  it('counts tags and sorts namespaced chips ahead of bare ones', () => {
    const counts = tagCounts([
      rule('zzz', 'fam:scalper'),
      rule('fam:scalper', 'risk:high'),
      rule(),
    ]);
    expect(counts).toEqual([
      { tag: 'fam:scalper', count: 2 },
      { tag: 'risk:high', count: 1 },
      { tag: 'zzz', count: 1 },
    ]);
  });

  it('reads the namespace prefix', () => {
    expect(tagNamespace('fam:scalper')).toBe('fam');
    expect(tagNamespace('scalper')).toBeNull();
    // A leading colon is not a namespace (no prefix before it).
    expect(tagNamespace(':scalper')).toBeNull();
  });
});

describe('url codec', () => {
  it('round-trips both sides', () => {
    const f: TagFilterState = { include: ['fam:scalper'], exclude: ['stage:experiment'] };
    const params = new URLSearchParams(serializeTagFilter(f));
    expect(parseTagFilter(params)).toEqual(f);
  });

  it('omits empty sides so a cleared filter leaves no stale param', () => {
    expect(serializeTagFilter(EMPTY_TAG_FILTER)).toEqual({});
    expect(serializeTagFilter({ include: ['a'], exclude: [] })).toEqual({ tags: 'a' });
  });

  it('tolerates junk in the url', () => {
    const params = new URLSearchParams('tags=a,,b,%20a%20&notags=');
    expect(parseTagFilter(params)).toEqual({ include: ['a', 'b'], exclude: [] });
  });
});
