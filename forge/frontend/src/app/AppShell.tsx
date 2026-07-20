import { useEffect } from 'react';
import { NavLink, Outlet, useLocation } from 'react-router-dom';
import clsx from 'clsx';
import { IngestToggle } from '@shared/components/IngestToggle';
import { useQuoteAssetsQuery } from '@shared/store/endpoints';
import { documentTitleFor, NAV } from './nav';

function SolPrice() {
  const { data } = useQuoteAssetsQuery(undefined, {
    pollingInterval: 60_000,
    skipPollingIfUnfocused: true,
  });
  const native = data?.find((q) => q.is_native);
  if (!native?.usd_rate) return null;
  return (
    <span className="badge badge-neutral mono">SOL ${native.usd_rate.toFixed(2)}</span>
  );
}

export function AppShell() {
  const { pathname } = useLocation();

  useEffect(() => {
    document.title = documentTitleFor(pathname);
  }, [pathname]);

  return (
    <div className="flex min-h-screen">
      <aside className="w-56 shrink-0 border-r border-[var(--color-line)] bg-[var(--color-panel)] flex flex-col">
        <div className="px-4 py-4 border-b border-[var(--color-line)]">
          <div className="text-sm font-semibold tracking-wide">Launch Platform</div>
          <div className="text-[0.7rem] muted uppercase tracking-wider">live · pump.fun</div>
        </div>
        <nav className="flex-1 p-2 space-y-0.5">
          {NAV.map((n) => (
            <NavLink
              key={n.to}
              to={n.to}
              end={n.end}
              className={({ isActive }) =>
                clsx(
                  'flex items-center gap-2.5 rounded-md px-3 py-2 text-sm transition-colors',
                  isActive
                    ? 'bg-[var(--color-accent)] text-white font-medium'
                    : 'muted hover:bg-white/5 hover:text-[var(--color-ink)]',
                )
              }
            >
              <span className="w-4 text-center opacity-80">{n.icon}</span>
              {n.label}
            </NavLink>
          ))}
        </nav>
        <div className="p-3 border-t border-[var(--color-line)] text-[0.7rem] muted">
          EC2 · 2vCPU / 4GB
        </div>
      </aside>

      <div className="flex-1 min-w-0 flex flex-col">
        <header className="flex items-center justify-end gap-3 px-6 py-3 border-b border-[var(--color-line)] bg-[var(--color-panel)]/60">
          <SolPrice />
          <IngestToggle />
        </header>
        <main className="flex-1 min-w-0 p-6 space-y-4 max-w-[1400px] w-full mx-auto">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
