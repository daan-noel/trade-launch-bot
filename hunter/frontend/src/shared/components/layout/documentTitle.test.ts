import { describe, expect, it } from 'vitest';
import { documentTitleFor, formatDocumentTitle, resolveNavPageLabel } from './documentTitle';
import type { NavConfig } from './navTypes';

const nav: NavConfig = {
  identity: { appTitle: 'Hunter Live', subtitle: 'Live Trading', badge: 'LIVE' },
  items: [
    { kind: 'item', to: '/', label: 'Home' },
    {
      kind: 'group',
      label: 'Tokens',
      basePath: '/tokens',
      items: [
        { to: '/tokens', label: 'All tokens' },
        { to: '/tokens/sync', label: 'Sync token' },
      ],
    },
    { kind: 'item', to: '/positions', label: 'Positions' },
  ],
};

describe('resolveNavPageLabel', () => {
  it('matches exact leaf paths', () => {
    expect(resolveNavPageLabel('/positions', nav.items)).toBe('Positions');
    expect(resolveNavPageLabel('/tokens/sync', nav.items)).toBe('Sync token');
  });

  it('prefers the longer of two overlapping prefixes', () => {
    expect(resolveNavPageLabel('/tokens', nav.items)).toBe('All tokens');
  });

  it('returns null for unknown routes', () => {
    expect(resolveNavPageLabel('/nope', nav.items)).toBeNull();
  });
});

describe('formatDocumentTitle', () => {
  it('page-first; home collapses to bare app title', () => {
    expect(formatDocumentTitle('Hunter Live', 'Home')).toBe('Hunter Live');
    expect(formatDocumentTitle('Hunter Live', 'Armed')).toBe('Armed · Hunter Live');
  });
});

describe('documentTitleFor', () => {
  it('wires nav + identity', () => {
    expect(documentTitleFor('/', nav)).toBe('Hunter Live');
    expect(documentTitleFor('/tokens/sync', nav)).toBe('Sync token · Hunter Live');
    expect(documentTitleFor('/missing', nav)).toBe('Not Found · Hunter Live');
  });
});
