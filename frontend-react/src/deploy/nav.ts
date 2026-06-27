import type { NavConfig } from 'components/layout/navTypes';

/**
 * Deploy (LIVE box) nav. Teal accent + the live-mode kill switch (wired in the
 * layout's header slot). Tokens stays prominent — it's the live-ingest monitor.
 */
export const deployNav: NavConfig = {
  accent: 'teal',
  items: [
    { kind: 'item', to: '/', label: 'Home' },
    { kind: 'item', to: '/dashboard', label: 'Dashboard' },
    {
      kind: 'group',
      label: 'Tokens',
      basePath: '/token',
      items: [
        { to: '/tokens', label: 'All tokens' },
        { to: '/token/sync', label: 'Sync token' },
      ],
    },
    { kind: 'item', to: '/transactions', label: 'Transactions' },
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
