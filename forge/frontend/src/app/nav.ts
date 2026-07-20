/** Sidebar + tab-title SSOT for the forge live operator UI. */
export const APP_TITLE = 'Forge Live';

export const NAV: { to: string; label: string; icon: string; end?: boolean }[] = [
  { to: '/', label: 'Dashboard', icon: '◉', end: true },
  { to: '/launch', label: 'Launch Console', icon: '🚀' },
  { to: '/launches', label: 'Launched Tokens', icon: '≡' },
  { to: '/wallets', label: 'Wallet Pool', icon: '◈' },
  { to: '/templates', label: 'Launch Templates', icon: '⚙' },
  { to: '/metadata', label: 'Metadata', icon: '⬡' },
];

/** Shorten a mint for the tab (full base58 is unreadable at tab width). */
function shortMint(mint: string): string {
  return mint.length > 10 ? `${mint.slice(0, 4)}…${mint.slice(-4)}` : mint;
}

/**
 * Resolve the page segment for `pathname`. Nav labels are the SSOT; the token
 * detail drill-down (no nav row) uses a shortened mint.
 */
export function resolveForgePageLabel(pathname: string): string | null {
  const path = pathname.length > 1 && pathname.endsWith('/') ? pathname.slice(0, -1) : pathname;

  const exact = NAV.find((n) => n.to === path);
  if (exact) return exact.label;

  if (path.startsWith('/tokens/')) {
    const mint = path.slice('/tokens/'.length);
    return mint ? shortMint(decodeURIComponent(mint)) : 'Token';
  }

  return null;
}

/** Page-first tab title; home/dashboard keeps the bare app identity. */
export function documentTitleFor(pathname: string): string {
  const page = resolveForgePageLabel(pathname) ?? 'Not Found';
  if (page === 'Dashboard') return APP_TITLE;
  return `${page} · ${APP_TITLE}`;
}
