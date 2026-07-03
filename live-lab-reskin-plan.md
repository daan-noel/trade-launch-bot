# live / lab frontend re-skin plan

> **Status (2026-07-03):** Phases 0–3, 5, 6 **done** — lab accent is **cyan `#06b6d4`**
> (violet rejected, then blue `#3b82f6` felt too far from teal; cyan is one hue-step over).
> `npm run build` clean (both trees typecheck). Phase 4 (home
> content divergence) **skipped by decision** — the chrome re-skin already makes the
> apps distinguishable; Home stays the shared placeholder. **Re-skin complete.**

Make the **live** and **lab** SPAs visually distinct at a glance so you always know
which app you're in — primarily an **error-prevention** measure (never mistake the
lab sandbox for the live-trading cockpit), secondarily identity.

## Scope (decided)

- **Re-skin only.** No layout/nav-model change. Diverge: background tint, accent,
  logo/identity strings, and (light) home content.
- **Strict SSOT.** Diverge **only** via CSS theme tokens + data-driven header
  content. **Never fork** `components/ui/*`, `DataTable`, or any shared component.
- **live is the EC2-shipped app** → keep its palette essentially unchanged (lowest
  risk). All meaningful visual change lands on **lab**.

## Non-goals

- No sidebar / status-strip / dashboard restructuring (that was the rejected
  "diverge layout" scope — revisit later if desired).
- No new component variants or forks.
- No change to the build split, routing, or data layer.

## Mechanism (one idea, everything hangs off it)

Tailwind v4 compiles `bg-bg`, `text-primary`, `border-border`, … to
`var(--color-*)`. So overriding those CSS variables under a higher-specificity
`:root[data-app='lab']` selector **re-skins every shared component at once** — no
forks. `@theme` stays live's base; lab is one override block.

- Entry HTML tags the root: `index.html` → `data-app="live"`, `lab.html` → `data-app="lab"`.
- Shared `src/index.css` holds the single lab override block.

## Proposed palette

**live** — unchanged (neutral near-black + teal `--color-primary: #13ceaf`).

**lab** — cool slate + violet, clearly "analytical / cold":

| token | live (base) | lab override |
| --- | --- | --- |
| `--color-bg` | `#0f0f0f` | `#0c0e13` |
| `--color-bg-panel` | `#1a1a1a` | `#13161d` |
| `--color-bg-card` | `#222222` | `#1a1e27` |
| `--color-bg-hover` | `#2a2a2a` | `#222735` |
| `--color-border` | `#2a2a2a` | `#252a37` |
| `--color-primary` | `#13ceaf` (teal) | `#06b6d4` (cyan) |

Folding the lab color into `--color-primary` means the per-mode class strings in
`lib/accent.ts` become redundant → **net code deletion** (see Phase 3).

## Identity (data-driven through NavConfig — Header stays shared)

| | live | lab |
| --- | --- | --- |
| subtitle | `Live Trading` | `Research & Backtesting` |
| badge | `LIVE` (+ pulse dot) | `LAB` |
| glyph (optional) | `◈` | `◇` |

---

## Phase 0 — Baseline & guardrails

- [ ] `cd frontend-react; npm run build` on a clean tree → capture current pass state.
- [ ] Screenshot current live + lab home/tokens pages for before/after comparison.
- [ ] Confirm Tailwind v4 emits `@theme` vars to `:root` (so `:root[data-app=…]`
      wins on specificity) — quick check in built CSS or a throwaway override.

## Phase 1 — Per-app theme tokens (the core reskin)

- [ ] `index.html`: `<html lang="en" data-app="live">`.
- [ ] `lab.html`: `<html lang="en" data-app="lab">`.
- [ ] `src/index.css`: add `:root[data-app='lab'] { … }` override block with the
      lab palette above (bg/panel/card/hover/border/primary).
- [ ] Verify live is byte-for-byte visually unchanged (base tokens untouched).
- [ ] Run both dev servers (`npm run dev`) and eyeball: lab reads cool/violet,
      live reads neutral/teal; tables, buttons, cards all followed the tokens.

## Phase 2 — Identity strings via NavConfig

- [ ] `navTypes.ts`: add `identity: { subtitle: string; badge: string; glyph?: string }`
      to `NavConfig`.
- [ ] `live/nav.ts`: `identity: { subtitle: 'Live Trading', badge: 'LIVE', glyph: '◈' }`.
- [ ] `lab/nav.ts`: `identity: { subtitle: 'Research & Backtesting', badge: 'LAB', glyph: '◇' }`.
- [ ] `Header.tsx`: render `nav.identity.subtitle` (replace hardcoded "Solana Bot"),
      `nav.identity.glyph ?? '◈'`, and a small badge chip using `nav.identity.badge`.
- [ ] live badge gets a subtle pulse dot (CSS keyframe, live-only via the badge text
      or an `data-app` guard) — optional, keep if cheap.

## Phase 3 — Collapse the accent special-case (SSOT cleanup)

Now that `--color-primary` is violet under lab, the nav-active + logo highlights can
use `primary` utilities directly and auto-adapt per app.

- [ ] `Header.tsx`: replace `accentClasses[nav.accent].navActive` with a single
      `primary`-based class (`bg-primary/12 text-primary`; for the inset shadow use
      `shadow-[inset_0_1px_0_color-mix(in_srgb,var(--color-primary)_15%,transparent)]`
      or drop it).
- [ ] `Header.tsx`: replace `accentClasses[nav.accent].logo` with a single
      `primary`-based class.
- [ ] Delete `src/shared/lib/accent.ts`.
- [ ] `navTypes.ts`: remove `accent: AccentColor` and its import.
- [ ] `live/nav.ts` / `lab/nav.ts`: remove the `accent:` field.
- [ ] Grep for any remaining `accentClasses` / `AccentColor` importers → none.

## Phase 4 — Home content divergence (light)

- [ ] Locate each app's Home route (`src/pages/home/HomePage.tsx` + per-app `App.tsx`
      routing) and confirm whether Home is shared or per-app.
- [ ] Give lab Home a research-framed heading/blurb + quick links (sweeps, analysis,
      lake freshness) and live Home an ops-framed heading (positions, wallet,
      strategies). Keep it to copy + existing components — **no new components**.
- [ ] If Home is currently a single shared page, split the *content* per app while
      reusing shared building blocks (do not fork shared UI primitives).

## Phase 5 — Hardcoded-color audit (won't auto-inherit)

Chart.js / inline-hex components read fixed colors, not CSS vars, so they don't
follow the reskin.

- [ ] Grep `#[0-9a-fA-F]{6}` under `src/shared/components/token-price-chart/` and
      `constants.ts` (known palette incl. `#8b5cf6`).
- [ ] Confirm lab pages that use literal `violet` (e.g. `Swing1DetectPage.tsx`
      latched-leg tint `bg-violet-400/15`, `text-violet-300`) still read correctly
      now that `primary` is also violet — adjust only if they visually clash.
- [ ] Decide per chart: leave as-is (acceptable — charts are semantic palettes) or
      wire a couple of key colors to `var(--color-primary)`. Default: leave; log any
      deferred items here.

## Phase 6 — Verify & document

- [ ] `npm run build` clean (tsc checks BOTH trees + vite live build).
- [ ] No extra re-render on SOL/USD tick or live-trade stream (reskin is
      CSS/data-only, so this should hold — spot-check).
- [ ] After/before screenshots side by side; confirm instant "which app" distinction.
- [ ] Update `@arch/frontend.md`: note the `data-app` theming mechanism + that live/lab
      diverge by token set, not component forks; drop the `accent.ts` reference if deleted.
- [ ] Update `CLAUDE.md` frontend section if the accent mechanism description changed.

## Risks / open questions

- **Specificity:** relies on `:root[data-app='lab']` beating `@theme`'s `:root`.
  Verified in Phase 0; if Tailwind changes emission target, fall back to a per-entry
  CSS import instead of a shared override block.
- **Charts unaffected:** accepted (Phase 5) — they're semantic palettes, not chrome.
- **`primary`-is-violet collisions in lab:** the swing "latched leg" violet is a
  fixed semantic highlight; confirm it still stands out against a now-violet primary.
- **live untouched guarantee:** base `@theme` must not be edited in Phase 1 — only
  the `data-app='lab'` block is added.

## Rollback

Pure additive/CSS + data changes. Revert = remove the `data-app` attrs, the lab
override block, and the NavConfig `identity` field (restore `accent.ts` if Phase 3
ran). No data/build-structure risk.
