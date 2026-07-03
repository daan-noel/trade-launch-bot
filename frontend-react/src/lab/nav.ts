import type { NavConfig } from 'components/layout/navTypes';

/**
 * Lab (workstation) nav. Violet accent, no live-mode toggle (there is no
 * ingest/trading to switch). Adds the Analysis pages + strategy authoring +
 * grouped sweeps; omits the live-only live-trading entries.
 */
export const labNav: NavConfig = {
  accent: 'violet',
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
      basePath: '/strategies/tpsl',
      items: [
        { to: '/strategies/tpsl1', label: 'TP / SL Sniper 1' },
        { to: '/strategies/tpsl2', label: 'TP / SL Sniper 2' },
        { to: '/strategies/swing1', label: 'Swing 1' },
      ],
    },

    { kind: 'item', to: '/profiles', label: 'Profiles' },
    { kind: 'item', to: '/settings', label: 'Settings' },
  ],
};
