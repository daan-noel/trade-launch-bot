import { lazy } from 'react';
import { BrowserRouter, Navigate, Route, Routes, useLocation } from 'react-router-dom';
import { AppProviders } from 'context/AppProviders';
import { AppLayout } from 'components/layout/AppLayout';
import { RouteErrorBoundary } from 'components/ui/ErrorBoundary';
import { LiveModeControl } from '@live/components/LiveModeControl';
import { RunningTasksIndicator } from '@live/components/RunningTasksIndicator';
import { usePositionNotifications } from '@live/hooks/usePositionNotifications';
import { useLiveStatusBootstrap } from '@live/hooks/useLiveStatusBootstrap';
import { usePortfolioRealtime } from '@live/hooks/usePortfolioRealtime';
import { useWalletMarksLive } from '@live/hooks/useWalletMarksLive';
import { useTokenTradesLiveBootstrap } from 'hooks/useTokenTradesLive';
import { liveNav } from './nav';

// Code-split each route into its own chunk. Pages export named (not default)
// components, so map the named export onto `default` in each lazy() call.
const HomePage = lazy(() => import('@live/pages/home/LiveHomePage').then((m) => ({ default: m.LiveHomePage })));
const TokensPage = lazy(() => import('pages/tokens/TokensPage').then((m) => ({ default: m.TokensPage })));
const SyncTokenPage = lazy(() => import('@live/pages/tokens/SyncTokenPage').then((m) => ({ default: m.SyncTokenPage })));
const MyWalletPage = lazy(() => import('@live/pages/profiles/MyWalletPage').then((m) => ({ default: m.MyWalletPage })));
const ProfilesPage = lazy(() => import('pages/profiles/ProfilesPage').then((m) => ({ default: m.ProfilesPage })));
const SettingsPage = lazy(() =>
  import('@live/pages/settings/LiveSettingsPage').then((m) => ({ default: m.LiveSettingsPage })),
);
const RulesPage = lazy(() => import('@live/pages/strategies/RulesPage').then((m) => ({ default: m.RulesPage })));
const FingerprintsPage = lazy(() => import('@live/pages/strategies/FingerprintsPage').then((m) => ({ default: m.FingerprintsPage })));
const ConsolePage = lazy(() => import('@live/pages/console/ConsolePage').then((m) => ({ default: m.ConsolePage })));
const PortfolioPage = lazy(() =>
  import('@live/pages/portfolio/PortfolioPage').then((m) => ({ default: m.PortfolioPage })),
);
const RuleAnalyzePage = lazy(() =>
  import('@live/pages/strategies/RuleAnalyzePage').then((m) => ({ default: m.RuleAnalyzePage })),
);
const NotFoundPage = lazy(() => import('pages/not-found/NotFoundPage').then((m) => ({ default: m.NotFoundPage })));

/** Mounts Live Status SSOT + portfolio/chart live pipes + position toasts. */
function NotificationMount() {
  useLiveStatusBootstrap();
  usePortfolioRealtime();
  useWalletMarksLive();
  useTokenTradesLiveBootstrap();
  usePositionNotifications();
  return null;
}

/** Preserve query string when redirecting renamed routes. */
function RedirectPreserve({ to }: { to: string }) {
  const { search } = useLocation();
  return <Navigate to={`${to}${search}`} replace />;
}

export default function App() {
  return (
    <BrowserRouter>
      <AppProviders>
        <RouteErrorBoundary variant="root">
          <Routes>
            <Route
              element={
                <AppLayout
                  nav={liveNav}
                  rightSlot={<LiveModeControl />}
                  beforeMain={<NotificationMount />}
                  footer={<RunningTasksIndicator />}
                />
              }
            >
              <Route index element={<HomePage />} />
              <Route path="tokens" element={<TokensPage />} />
              <Route path="tokens/sync" element={<SyncTokenPage />} />
              <Route path="token/sync" element={<RedirectPreserve to="/tokens/sync" />} />
              <Route path="console" element={<ConsolePage />} />
              {/* Console replaced Floor + Trade — old routes redirect with query
                  preserved (`/trade?mint=X` prefills the manual-trade panel). */}
              <Route path="floor" element={<RedirectPreserve to="/console" />} />
              <Route path="trade" element={<RedirectPreserve to="/console" />} />
              <Route path="ops" element={<RedirectPreserve to="/console" />} />
              <Route path="portfolio" element={<PortfolioPage />} />
              <Route path="positions" element={<RedirectPreserve to="/console" />} />
              <Route path="live-trading" element={<RedirectPreserve to="/console" />} />
              <Route path="strategies/armed" element={<Navigate to="/console" replace />} />
              <Route path="strategies/monitor" element={<Navigate to="/console" replace />} />
              <Route path="strategies/rules" element={<RulesPage />} />
              <Route path="strategies/rules/:ruleId" element={<RuleAnalyzePage />} />
              <Route path="strategies/fingerprints" element={<FingerprintsPage />} />
              <Route path="wallet" element={<MyWalletPage />} />
              <Route path="profiles" element={<ProfilesPage />} />
              <Route path="settings" element={<SettingsPage />} />
              <Route path="*" element={<NotFoundPage />} />
            </Route>
          </Routes>
        </RouteErrorBoundary>
      </AppProviders>
    </BrowserRouter>
  );
}
