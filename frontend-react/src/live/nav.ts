import type { NavConfig } from 'components/layout/navTypes';

/**
 * Live (LIVE box) nav. Teal accent + the live-mode kill switch (wired in the
 * layout's header slot). Tokens stays prominent — it's the live-ingest monitor.
 */
export const liveNav: NavConfig = {
  accent: 'teal',
  items: [
    { kind: 'item', to: '/', label: 'Home' },
    {
      kind: 'group',
      label: 'Tokens',
      basePath: '/token',
      items: [
        { to: '/tokens', label: 'All tokens' },
        { to: '/token/sync', label: 'Sync token' },
      ],
    },
    {
      kind: 'group',
      label: 'Strategies',
      basePath: '/strategies',
      items: [
        { to: '/strategies/tpsl1', label: 'TP / SL Sniper 1' },
        { to: '/strategies/tpsl2', label: 'TP / SL Sniper 2' },
      ],
    },
    {
      kind: 'group',
      label: 'Profiles',
      basePath: '/profiles',
      items: [
        { to: '/profiles/mine', label: 'My wallets' },
        { to: '/profiles/other', label: 'Other profiles' },
      ],
    },
    { kind: 'item', to: '/settings', label: 'Settings' },
  ],
};
