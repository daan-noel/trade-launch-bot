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
      items: [
        { to: '/analysis/trader', label: 'Trader Analysis' },
        { to: '/analysis/swing-detection', label: 'Swing detection' },
        { to: '/analysis/swing1-detect', label: 'swing1 detect' },
      ],
    },
    {
      kind: 'group',
      label: 'Grouped Sweep',
      basePath: '/strategies/grouped-sweep',
      items: [
        { to: '/strategies/grouped-sweep-tpsl1', label: 'TPSL1' },
        { to: '/strategies/grouped-sweep-tpsl2', label: 'TPSL2' },
        { to: '/strategies/grouped-sweep-swing1', label: 'swing1' },
      ],
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
        { to: '/strategies/tpsl1', label: 'TP / SL Sniper 1 (legacy)' },
        { to: '/strategies/tpsl2', label: 'TP / SL Sniper 2 (legacy)' },
        { to: '/strategies/swing1', label: 'Swing 1 (legacy)' },
      ],
    },

    { kind: 'item', to: '/profiles', label: 'Profiles' },
    { kind: 'item', to: '/settings', label: 'Settings' },
  ],
};
