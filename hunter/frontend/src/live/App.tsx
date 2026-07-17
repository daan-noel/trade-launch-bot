import { lazy, Suspense } from 'react';
import { BrowserRouter, Route, Routes } from 'react-router-dom';
import { AppProviders } from 'context/AppProviders';
import { AppLayout } from 'components/layout/AppLayout';
import { RouteErrorBoundary } from 'components/ui/ErrorBoundary';
import { SuspenseFallback } from 'components/ui/SuspenseFallback';
import { LiveModeControl } from '@live/components/LiveModeControl';
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
const Tpsl1Page = lazy(() => import('@live/pages/strategies/TpslPage').then((m) => ({ default: () => m.TpslPage({ strategy: 'tpsl1' }) })));
const Tpsl2Page = lazy(() => import('@live/pages/strategies/TpslPage').then((m) => ({ default: () => m.TpslPage({ strategy: 'tpsl2' }) })));
const Swing1Page = lazy(() => import('@live/pages/strategies/Swing1Page').then((m) => ({ default: m.Swing1Page })));
const RulesPage = lazy(() => import('@live/pages/strategies/RulesPage').then((m) => ({ default: m.RulesPage })));
const FingerprintsPage = lazy(() => import('@live/pages/strategies/FingerprintsPage').then((m) => ({ default: m.FingerprintsPage })));
const LiveTradingPage = lazy(() => import('@live/pages/strategies/LiveTradingPage').then((m) => ({ default: m.LiveTradingPage })));
const TradePage = lazy(() => import('@live/pages/trade/TradePage').then((m) => ({ default: m.TradePage })));
const NotFoundPage = lazy(() => import('pages/not-found/NotFoundPage').then((m) => ({ default: m.NotFoundPage })));

/** Mounts the global position-notification toasts (live-only). */
function NotificationMount() {
  usePositionNotifications();
  return null;
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
                  />
                }
              >
                <Route index element={<HomePage />} />
                <Route path="tokens" element={<TokensPage />} />
                <Route path="token/sync" element={<SyncTokenPage />} />
                <Route path="strategies/rules" element={<RulesPage />} />
                <Route path="strategies/fingerprints" element={<FingerprintsPage />} />
                <Route path="strategies/tpsl1" element={<Tpsl1Page />} />
                <Route path="strategies/tpsl2" element={<Tpsl2Page />} />
                <Route path="strategies/swing1" element={<Swing1Page />} />
                <Route path="trade" element={<TradePage />} />
                <Route path="live-trading" element={<LiveTradingPage />} />
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
