import type { NavConfig } from 'components/layout/navTypes';

/**
 * Lab (workstation) nav. Cyan primary (swapped in via `data-app="lab"`), no
 * live-mode toggle (there is no ingest/trading to switch). Adds the Analysis
 * pages + strategy authoring + grouped sweeps; omits the live-only live-trading
 * entries. A calm `LAB` badge (no pulse) marks the research sandbox.
 */
export const labNav: NavConfig = {
  identity: { subtitle: 'Research & Backtesting', badge: 'LAB', glyph: '◇' },
  items: [
    { kind: 'item', to: '/', label: 'Home' },
    { kind: 'item', to: '/creation-stats', label: 'Creation Stats' },
    {
      kind: 'group',
      label: 'Tokens',
      basePath: '/token',
      items: [{ to: '/tokens', label: 'All tokens' }],
    },
    {
      kind: 'group',
      label: 'Analysis',
      basePath: '/analysis',
      items: [{ to: '/analysis/trader', label: 'Trader Analysis' }],
    },
    {
      kind: 'group',
      label: 'Strategies',
      basePath: '/strategies',
      items: [
        { to: '/strategies/rules', label: 'Rules' },
        { to: '/strategies/fingerprints', label: 'Fingerprints' },
        { to: '/strategies/simulate', label: 'Simulate' },
        { to: '/strategies/metric-panes', label: 'Metric panes' },
        { to: '/strategies/sweep', label: 'Grouped sweep' },
      ],
    },

    { kind: 'item', to: '/profiles', label: 'Profiles' },
    { kind: 'item', to: '/settings', label: 'Settings' },
  ],
};
