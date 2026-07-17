import type { NavConfig } from 'components/layout/navTypes';

/**
 * Lab (workstation) nav. Cyan primary (swapped in via `data-app="lab"`), no
 * live-mode toggle. Single-destination groups are flattened to leaf links.
 * Metric panes live inside Tokens detail (not a peer Strategies page).
 */
export const labNav: NavConfig = {
  identity: { subtitle: 'Research & Backtesting', badge: 'LAB', glyph: '◇' },
  items: [
    { kind: 'item', to: '/', label: 'Home' },
    { kind: 'item', to: '/creation-stats', label: 'Creation Stats' },
    { kind: 'item', to: '/tokens', label: 'Tokens' },
    { kind: 'item', to: '/analysis/trader', label: 'Trader Analysis' },
    {
      kind: 'group',
      label: 'Strategies',
      basePath: '/strategies',
      items: [
        { to: '/strategies/rules', label: 'Rules' },
        { to: '/strategies/fingerprints', label: 'Fingerprints' },
        { to: '/strategies/simulate', label: 'Simulate' },
        { to: '/strategies/sweep', label: 'Grouped sweep' },
        { to: '/strategies/replay', label: 'Replay viewer' },
      ],
    },
    { kind: 'item', to: '/profiles', label: 'Profiles' },
    { kind: 'item', to: '/settings', label: 'Settings' },
  ],
};
