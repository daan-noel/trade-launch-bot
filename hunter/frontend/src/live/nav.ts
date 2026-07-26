import type { NavConfig } from 'components/layout/navTypes';

/**
 * Live (LIVE box) nav. Teal primary (base theme) + the live-mode kill switch
 * (wired in the layout's header slot). Money surfaces: Console / Portfolio /
 * Wallet; strategy authoring under Strategies. The `LIVE` badge pulses as a cue.
 */
export const liveNav: NavConfig = {
  identity: {
    appTitle: 'Hunter Live',
    subtitle: 'Live Trading',
    badge: 'LIVE',
    glyph: '◈',
    pulse: true,
  },
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
    {
      kind: 'group',
      label: 'Strategies',
      basePath: '/strategies',
      items: [
        { to: '/strategies/rules', label: 'Rules' },
        { to: '/strategies/fingerprints', label: 'Fingerprints' },
      ],
    },
    { kind: 'item', to: '/console', label: 'Console' },
    { kind: 'item', to: '/portfolio', label: 'Portfolio' },
    { kind: 'item', to: '/wallet', label: 'Wallet' },
    { kind: 'item', to: '/profiles', label: 'Profiles' },
    { kind: 'item', to: '/settings', label: 'Settings' },
  ],
};
