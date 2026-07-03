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
 * Per-app identity rendered in the shared `Header` logo block. Carries the
 * strings/flags that differ between the live cockpit and the lab workbench so
 * the Header stays single-source (data in, no per-mode branch). The per-app
 * *color* is not here — it comes from the `--color-primary` theme token, which
 * `index.css` swaps under `data-app="lab"`, so `text-primary`/`bg-primary`
 * auto-adapt (teal on live, violet on lab).
 */
export interface AppIdentity {
  /** Short line under the app name stating the app's purpose. */
  subtitle: string;
  /** Corner badge chip text (e.g. `LIVE` / `LAB`). */
  badge: string;
  /** Logo glyph; defaults to `◈` when omitted. */
  glyph?: string;
  /** Pulse the badge dot — a live-cockpit "this is armed" affordance. */
  pulse?: boolean;
}

/**
 * Per-mode nav configuration consumed by the shared `Header`. The split replaces
 * the old runtime `useCapabilities` gating with a static, build-time nav list:
 * each mode's `App` passes its own config, so the live build literally cannot
 * render an analysis route and vice-versa.
 */
export interface NavConfig {
  identity: AppIdentity;
  items: NavEntry[];
}
