import { describe, expect, it } from 'vitest';

import { veteranRosterFromConfig } from './registry';

// The three states the fingerprint form has to tell apart. Collapsing "absent" into
// "empty" would hide the only case where a rule on m_bundle can never fire.
describe('veteranRosterFromConfig', () => {
  it('reports an absent m_bundle key as unconfigured', () => {
    expect(veteranRosterFromConfig(undefined)).toBeNull();
    expect(veteranRosterFromConfig({})).toBeNull();
    expect(veteranRosterFromConfig({ m_flow_split: { volume_ix_patterns: [['a']] } })).toBeNull();
  });

  it('keeps a configured-but-empty roster distinct from an absent one', () => {
    expect(veteranRosterFromConfig({ m_bundle: {} })).toEqual({
      minLaunches: null,
      wallets: [],
    });
  });

  it('reads the bar and the wallet list', () => {
    expect(
      veteranRosterFromConfig({
        m_bundle: { veteran_min_launches: 25, veteran_wallets: ['A', 'B'] },
      }),
    ).toEqual({ minLaunches: 25, wallets: ['A', 'B'] });
  });

  it('drops non-string wallets rather than rendering them', () => {
    const r = veteranRosterFromConfig({
      m_bundle: { veteran_min_launches: 'lots', veteran_wallets: ['A', 7, null] },
    });
    expect(r).toEqual({ minLaunches: null, wallets: ['A'] });
  });
});
