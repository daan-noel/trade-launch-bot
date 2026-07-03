import type { NavConfig } from 'components/layout/navTypes';

/**
 * Live (LIVE box) nav. Teal primary (base theme) + the live-mode kill switch
 * (wired in the layout's header slot). Tokens stays prominent — it's the
 * live-ingest monitor. The `LIVE` badge pulses as a "this is armed" cue.
 */
export const liveNav: NavConfig = {
  identity: { subtitle: 'Live Trading', badge: 'LIVE', glyph: '◈', pulse: true },
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
        { to: '/strategies/swing1', label: 'Swing 1' },
      ],
    },
    { kind: 'item', to: '/wallet', label: 'My Wallet' },
    { kind: 'item', to: '/profiles', label: 'Profiles' },
    { kind: 'item', to: '/settings', label: 'Settings' },
  ],
};
