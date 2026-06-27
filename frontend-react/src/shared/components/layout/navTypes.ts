import type { AccentColor } from 'lib/accent';

/** A single top-level nav link. */
export interface NavLeaf {
  kind: 'item';
  to: string;
  label: string;
}

/** A nav dropdown grouping several links; `basePath` drives the active state. */
export interface NavGroup {
  kind: 'group';
  label: string;
  basePath: string;
  items: { to: string; label: string }[];
}

export type NavEntry = NavLeaf | NavGroup;

/**
 * Per-mode nav configuration consumed by the shared `Header`. The split replaces
 * the old runtime `useCapabilities` gating with a static, build-time nav list:
 * each mode's `App` passes its own config, so the deploy build literally cannot
 * render an analysis route and vice-versa.
 */
export interface NavConfig {
  accent: AccentColor;
  items: NavEntry[];
}
