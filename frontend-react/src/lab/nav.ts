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
      ],
    },
    {
      kind: 'group',
      label: 'Profiles',
      basePath: '/profiles',
      items: [{ to: '/profiles/other', label: 'Other profiles' }],
    },
    {
      kind: 'group',
      label: 'Strategies',
      basePath: '/strategies',
      items: [
        { to: '/strategies/tpsl1', label: 'TP / SL Sniper 1' },
        { to: '/strategies/tpsl2', label: 'TP / SL Sniper 2' },
        { to: '/strategies/grouped-sweep-tpsl1', label: 'Grouped Sweep · TPSL1' },
        { to: '/strategies/grouped-sweep-tpsl2', label: 'Grouped Sweep · TPSL2' },
      ],
    },
    { kind: 'item', to: '/settings', label: 'Settings' },
  ],
};
