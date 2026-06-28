import { lazy, Suspense } from 'react';
import { BrowserRouter, Navigate, Route, Routes } from 'react-router-dom';
import { AppProviders } from 'context/AppProviders';
import { AppLayout } from 'components/layout/AppLayout';
import { RouteErrorBoundary } from 'components/ui/ErrorBoundary';
import { SuspenseFallback } from 'components/ui/SuspenseFallback';
import { LiveModeControl } from '@live/components/LiveModeControl';
import { usePositionNotifications } from '@live/hooks/usePositionNotifications';
import { liveNav } from './nav';

// Code-split each route into its own chunk. Pages export named (not default)
// components, so map the named export onto `default` in each lazy() call.
const HomePage = lazy(() => import('pages/home/HomePage').then((m) => ({ default: m.HomePage })));
const TokensPage = lazy(() => import('pages/tokens/TokensPage').then((m) => ({ default: m.TokensPage })));
const SyncTokenPage = lazy(() => import('@live/pages/tokens/SyncTokenPage').then((m) => ({ default: m.SyncTokenPage })));
const MyWalletPage = lazy(() => import('@live/pages/profiles/MyWalletPage').then((m) => ({ default: m.MyWalletPage })));
const OtherProfilesPage = lazy(() => import('pages/profiles/OtherProfilesPage').then((m) => ({ default: m.OtherProfilesPage })));
const SettingsPage = lazy(() => import('pages/settings/SettingsPage').then((m) => ({ default: m.SettingsPage })));
const Tpsl1Page = lazy(() => import('@live/pages/strategies/TpslPage').then((m) => ({ default: () => m.TpslPage({ strategy: 'tpsl1' }) })));
const Tpsl2Page = lazy(() => import('@live/pages/strategies/TpslPage').then((m) => ({ default: () => m.TpslPage({ strategy: 'tpsl2' }) })));
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
                <Route path="strategies/tpsl1" element={<Tpsl1Page />} />
                <Route path="strategies/tpsl2" element={<Tpsl2Page />} />
                <Route path="wallet" element={<Navigate to="/profiles/mine" replace />} />
                <Route path="profiles/mine" element={<MyWalletPage />} />
                <Route path="profiles/other" element={<OtherProfilesPage />} />
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
