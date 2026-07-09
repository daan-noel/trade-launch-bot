import { Outlet } from 'react-router-dom';
import type { ReactNode } from 'react';
import { Header } from './Header';
import { RouteErrorBoundary } from 'components/ui/ErrorBoundary';
import type { NavConfig } from './navTypes';

/**
 * Shared app shell — chrome is identical across both builds; the mode-specific
 * bits are passed in as slots:
 * - `rightSlot`   → header right side (live: the live-mode kill switch)
 * - `beforeMain`  → mounted above the header (live: position notifications)
 * - `footer`      → below the content (analysis: the background-jobs indicator)
 *
 * Keeping these as slots (instead of conditionals) means the live build never
 * imports the lab-only footer and vice-versa.
 */
export function AppLayout({
  nav,
  rightSlot,
  beforeMain,
  footer,
}: {
  nav: NavConfig;
  rightSlot?: ReactNode;
  beforeMain?: ReactNode;
  footer?: ReactNode;
}) {
  return (
    <div className="flex min-h-screen flex-col bg-bg text-text">
      {beforeMain}
      <Header nav={nav} rightSlot={rightSlot} />
      <main className="flex-1 px-6 py-5 animate-[fade-in-up_0.25s_ease_both]">
        {/* A page-level throw degrades to a fallback card; the nav above stays live. */}
        <RouteErrorBoundary variant="page">
          <Outlet />
        </RouteErrorBoundary>
      </main>
      {footer}
    </div>
  );
}
