import type { NavConfig, NavEntry } from './navTypes';

/** Flatten nav leaves (items + group children) — the page-name SSOT. */
function navLeaves(items: NavEntry[]): { to: string; label: string }[] {
  const leaves: { to: string; label: string }[] = [];
  for (const entry of items) {
    if (entry.kind === 'item') leaves.push({ to: entry.to, label: entry.label });
    else leaves.push(...entry.items);
  }
  return leaves;
}

/**
 * Resolve the operator-facing page name for `pathname` from the nav config.
 * Prefers an exact match, then the longest prefix (so `/tokens/sync` wins over
 * `/tokens`). Returns `null` when nothing matches (caller maps that to 404).
 */
export function resolveNavPageLabel(pathname: string, items: NavEntry[]): string | null {
  const path = pathname.length > 1 && pathname.endsWith('/') ? pathname.slice(0, -1) : pathname;
  const leaves = navLeaves(items);

  const exact = leaves.find((l) => l.to === path);
  if (exact) return exact.label;

  let best: { to: string; label: string } | null = null;
  for (const leaf of leaves) {
    if (leaf.to !== '/' && path.startsWith(`${leaf.to}/`)) {
      if (!best || leaf.to.length > best.to.length) best = leaf;
    }
  }
  return best?.label ?? null;
}

/**
 * Tab title: page first (Chrome truncates from the right), then app identity.
 * Home collapses to just the app name so the default tab stays short.
 */
export function formatDocumentTitle(appTitle: string, pageLabel: string | null): string {
  if (!pageLabel || pageLabel === 'Home') return appTitle;
  return `${pageLabel} · ${appTitle}`;
}

/** Build the full document.title for the current route + nav config. */
export function documentTitleFor(pathname: string, nav: NavConfig): string {
  const page = resolveNavPageLabel(pathname, nav.items) ?? 'Not Found';
  return formatDocumentTitle(nav.identity.appTitle, page);
}
