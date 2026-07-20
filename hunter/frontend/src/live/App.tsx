import { lazy, Suspense } from 'react';
import { BrowserRouter, Navigate, Route, Routes, useLocation } from 'react-router-dom';
import { AppProviders } from 'context/AppProviders';
import { AppLayout } from 'components/layout/AppLayout';
import { RouteErrorBoundary } from 'components/ui/ErrorBoundary';
import { SuspenseFallback } from 'components/ui/SuspenseFallback';
import { LiveModeControl } from '@live/components/LiveModeControl';
import { RunningTasksIndicator } from '@live/components/RunningTasksIndicator';
import { usePositionNotifications } from '@live/hooks/usePositionNotifications';
import { liveNav } from './nav';

// Code-split each route into its own chunk. Pages export named (not default)
// components, so map the named export onto `default` in each lazy() call.
const HomePage = lazy(() => import('@live/pages/home/LiveHomePage').then((m) => ({ default: m.LiveHomePage })));
const TokensPage = lazy(() => import('pages/tokens/TokensPage').then((m) => ({ default: m.TokensPage })));
const SyncTokenPage = lazy(() => import('@live/pages/tokens/SyncTokenPage').then((m) => ({ default: m.SyncTokenPage })));
const MyWalletPage = lazy(() => import('@live/pages/profiles/MyWalletPage').then((m) => ({ default: m.MyWalletPage })));
const ProfilesPage = lazy(() => import('pages/profiles/ProfilesPage').then((m) => ({ default: m.ProfilesPage })));
const SettingsPage = lazy(() => import('pages/settings/SettingsPage').then((m) => ({ default: m.SettingsPage })));
const RulesPage = lazy(() => import('@live/pages/strategies/RulesPage').then((m) => ({ default: m.RulesPage })));
const FingerprintsPage = lazy(() => import('@live/pages/strategies/FingerprintsPage').then((m) => ({ default: m.FingerprintsPage })));
const MonitorPage = lazy(() => import('@live/pages/strategies/MonitorPage').then((m) => ({ default: m.MonitorPage })));
const LiveTradingPage = lazy(() => import('@live/pages/strategies/LiveTradingPage').then((m) => ({ default: m.LiveTradingPage })));
const TradePage = lazy(() => import('@live/pages/trade/TradePage').then((m) => ({ default: m.TradePage })));
const NotFoundPage = lazy(() => import('pages/not-found/NotFoundPage').then((m) => ({ default: m.NotFoundPage })));

/** Mounts the global position-notification toasts (live-only). */
function NotificationMount() {
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
          <Suspense fallback={<SuspenseFallback />}>
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
                <Route path="strategies/armed" element={<MonitorPage />} />
                <Route path="strategies/monitor" element={<RedirectPreserve to="/strategies/armed" />} />
                <Route path="strategies/rules" element={<RulesPage />} />
                <Route path="strategies/fingerprints" element={<FingerprintsPage />} />
                <Route path="positions" element={<LiveTradingPage />} />
                <Route path="live-trading" element={<RedirectPreserve to="/positions" />} />
                <Route path="trade" element={<TradePage />} />
                <Route path="wallet" element={<MyWalletPage />} />
                <Route path="profiles" element={<ProfilesPage />} />
                <Route path="settings" element={<SettingsPage />} />
                <Route path="*" element={<NotFoundPage />} />
              </Route>
            </Routes>
          </Suspense>
        </RouteErrorBoundary>
      </AppProviders>
    </BrowserRouter>
  );
}
