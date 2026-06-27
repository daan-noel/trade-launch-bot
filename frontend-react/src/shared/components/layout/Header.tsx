import { Link, NavLink, useLocation } from 'react-router-dom';
import { useEffect, type ReactNode } from 'react';
import { NavDropdown } from 'components/ui/NavDropdown';
import { PriceUnitToggle } from 'components/ui/PriceUnitToggle';
import { TimezoneSelect } from 'components/ui/TimezoneSelect';
import { useGetSolPriceQuery } from 'store/apiSlice';
import { usePriceUnit } from 'context/PriceUnitContext';
import { cn } from 'lib/cn';
import { accentClasses } from 'lib/accent';
import type { NavConfig } from './navTypes';

function NavItem({
  to,
  accentActive,
  children,
}: {
  to: string;
  accentActive: string;
  children: ReactNode;
}) {
  return (
    <NavLink
      to={to}
      end={to === '/'}
      className={({ isActive }) =>
        cn(
          'rounded-md px-3 py-1.5 text-[13px] font-medium transition-all duration-150',
          isActive ? accentActive : 'text-text-mid hover:bg-white/4 hover:text-text',
        )
      }
    >
      {children}
    </NavLink>
  );
}

/**
 * Shared app header. The nav set is fully data-driven from the per-mode
 * `NavConfig` (no more runtime `useCapabilities` gating), and the deploy-only
 * live-mode kill switch is injected via `rightSlot` so the analysis build never
 * imports the live-mode hooks. Shared everywhere: the SOL/USD mirror, timezone
 * selector, and price-unit toggle.
 */
export function Header({ nav, rightSlot }: { nav: NavConfig; rightSlot?: ReactNode }) {
  const location = useLocation();
  const { data: usdRate } = useGetSolPriceQuery();
  const { setUsdRate } = usePriceUnit();
  const accent = accentClasses[nav.accent];

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
          <span
            className={cn(
              'flex size-8 items-center justify-center rounded-lg text-sm ring-1 transition-colors',
              accent.logo,
            )}
          >
            ◈
          </span>
          <span className="flex flex-col leading-tight">
            <span className="text-sm font-semibold tracking-tight text-text">Meme Trading</span>
            <span className="text-[10px] font-medium tracking-wide text-text-dim uppercase">
              Solana Bot
            </span>
          </span>
        </Link>

        <div className="hidden h-5 w-px shrink-0 bg-white/8 md:block" aria-hidden />

        <nav className="flex min-w-0 flex-1 items-center gap-0.5 overflow-visible rounded-lg border border-white/6 bg-white/3 p-1">
          {nav.items.map((entry) =>
            entry.kind === 'item' ? (
              <NavItem key={entry.to} to={entry.to} accentActive={accent.navActive}>
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
          <TimezoneSelect />
          <PriceUnitToggle />
          {rightSlot}
        </div>
      </div>
    </header>
  );
}
