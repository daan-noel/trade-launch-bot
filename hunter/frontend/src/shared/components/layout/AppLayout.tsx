import { Suspense, useEffect, type ReactNode } from 'react';
import { Outlet, useLocation } from 'react-router-dom';
import { Header } from './Header';
import { RouteErrorBoundary } from 'components/ui/ErrorBoundary';
import { SuspenseFallback } from 'components/ui/SuspenseFallback';
import { documentTitleFor } from './documentTitle';
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
 *
 * Suspense wraps only `<Outlet />` so a lazy route chunk keeps the header/nav
 * mounted (an outer Suspense around `Routes` would blank the whole shell).
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
  const { pathname } = useLocation();

  // Tab title = nav page label + app identity (SSOT: nav.ts). Page-first so
  // Chrome's right-truncation keeps the differentiating segment visible.
  useEffect(() => {
    document.title = documentTitleFor(pathname, nav);
  }, [pathname, nav]);

  return (
    <div className="flex min-h-screen flex-col bg-bg text-text">
      {beforeMain}
      <Header nav={nav} rightSlot={rightSlot} />
      <main className="flex-1 px-6 py-5 animate-[fade-in-up_0.25s_ease_both]">
        {/* Page throw → fallback card; lazy chunk → LoadingState; nav stays live. */}
        <RouteErrorBoundary variant="page">
          <Suspense fallback={<SuspenseFallback />}>
            <Outlet />
          </Suspense>
        </RouteErrorBoundary>
      </main>
      {footer}
    </div>
  );
}
