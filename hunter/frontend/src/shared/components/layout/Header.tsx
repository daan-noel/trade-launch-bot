import { Link, NavLink, useLocation } from 'react-router-dom';
import { useEffect, type ReactNode } from 'react';
import { NavDropdown } from 'components/ui/NavDropdown';
import { PriceUnitToggle } from 'components/ui/PriceUnitToggle';
import { TimezoneSelect } from 'components/ui/TimezoneSelect';
import { useGetSolPriceQuery } from 'store/apiSlice';
import { useUsdRate } from 'context/PriceUnitContext';
import { useSseStatus } from 'hooks/useSseStatus';
import { cn } from 'lib/cn';
import type { NavConfig } from './navTypes';

/** Live health dot for the shared SSE stream — green open, amber retrying,
 * dim while first connecting. Title carries the word for screen readers. */
function SseStatusDot() {
  const status = useSseStatus();
  const { tone, label } =
    status === 'open'
      ? { tone: 'bg-emerald-400', label: 'Live stream connected' }
      : status === 'error'
        ? { tone: 'bg-amber-400 animate-pulse', label: 'Live stream reconnecting…' }
        : { tone: 'bg-white/30', label: 'Live stream connecting…' };
  return (
    <span
      role="status"
      aria-label={label}
      title={label}
      className="flex size-4 items-center justify-center"
    >
      <span className={cn('size-1.5 rounded-full', tone)} />
    </span>
  );
}

function NavItem({ to, children }: { to: string; children: ReactNode }) {
  return (
    <NavLink
      to={to}
      end={to === '/'}
      className={({ isActive }) =>
        cn(
          'rounded-md px-3 py-1.5 text-[13px] font-medium transition-all duration-150',
          // Active highlight rides `--color-primary`, so it's teal on live and
          // violet on lab with no per-mode branch.
          isActive
            ? 'bg-primary/12 text-primary shadow-[inset_0_1px_0_rgba(255,255,255,0.08)]'
            : 'text-text-mid hover:bg-white/4 hover:text-text',
        )
      }
    >
      {children}
    </NavLink>
  );
}

/**
 * Shared app header. The nav set is fully data-driven from the per-mode
 * `NavConfig` (no more runtime `useCapabilities` gating), and the live-only
 * live-mode kill switch is injected via `rightSlot` so the lab build never
 * imports the live-mode hooks. Shared everywhere: the SOL/USD mirror, timezone
 * selector, and price-unit toggle.
 */
export function Header({ nav, rightSlot }: { nav: NavConfig; rightSlot?: ReactNode }) {
  const location = useLocation();
  const { data: usdRate } = useGetSolPriceQuery();
  const { setUsdRate } = useUsdRate();
  const { subtitle, badge, glyph, pulse } = nav.identity;

  // Mirror the fetched SOL/USD rate into the price-unit context so USD display
  // works app-wide. The fetch itself is owned (and deduped) by the query above.
  useEffect(() => {
    if (usdRate !== undefined) setUsdRate(usdRate);
  }, [usdRate, setUsdRate]);

  return (
    <header className="sticky top-0 z-100 border-b border-white/6 bg-bg/75 backdrop-blur-xl backdrop-saturate-150">
      <div className="flex h-14 items-center gap-5 px-5">
        <Link
          to="/"
          className="group flex shrink-0 items-center gap-2.5 transition-opacity hover:opacity-90"
        >
          <span className="flex size-8 items-center justify-center rounded-lg bg-primary/10 text-sm text-primary ring-1 ring-primary/20 transition-colors group-hover:bg-primary/15">
            {glyph ?? '◈'}
          </span>
          <span className="flex flex-col leading-tight">
            <span className="flex items-center gap-1.5">
              <span className="text-sm font-semibold tracking-tight text-text">Meme Trading</span>
              <span className="inline-flex items-center gap-1 rounded bg-primary/10 px-1.5 py-px text-[9px] font-bold tracking-wider text-primary uppercase ring-1 ring-primary/25">
                {pulse && <span className="size-1 animate-pulse rounded-full bg-primary" />}
                {badge}
              </span>
            </span>
            <span className="text-[10px] font-medium tracking-wide text-text-dim uppercase">
              {subtitle}
            </span>
          </span>
        </Link>

        <div className="hidden h-5 w-px shrink-0 bg-white/8 md:block" aria-hidden />

        <nav className="flex min-w-0 flex-1 items-center gap-0.5 overflow-visible rounded-lg border border-white/6 bg-white/3 p-1">
          {nav.items.map((entry) =>
            entry.kind === 'item' ? (
              <NavItem key={entry.to} to={entry.to}>
                {entry.label}
              </NavItem>
            ) : (
              <NavDropdown
                key={entry.label}
                label={entry.label}
                isActive={location.pathname.startsWith(entry.basePath)}
                items={entry.items}
              />
            ),
          )}
        </nav>

        <div className="ml-auto flex shrink-0 items-center gap-2.5">
          <SseStatusDot />
          <TimezoneSelect />
          <PriceUnitToggle />
          {rightSlot}
        </div>
      </div>
    </header>
  );
}
