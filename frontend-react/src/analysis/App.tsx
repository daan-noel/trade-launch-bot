import { lazy, Suspense } from 'react';
import { BrowserRouter, Navigate, Route, Routes } from 'react-router-dom';
import { AppProviders } from 'context/AppProviders';
import { AppLayout } from 'components/layout/AppLayout';
import { RouteErrorBoundary } from 'components/ui/ErrorBoundary';
import { SuspenseFallback } from 'components/ui/SuspenseFallback';
import { BackgroundJobsProvider } from '@analysis/context/BackgroundJobsContext';
import { BackgroundJobsIndicator } from 'components/layout/BackgroundJobsIndicator';
import { GroupedCreationSection } from 'components/dashboard/GroupedCreationSection';
import { analysisNav } from './nav';

// Code-split each route into its own chunk. Pages export named (not default)
// components, so map the named export onto `default` in each lazy() call.
const HomePage = lazy(() => import('pages/home/HomePage').then((m) => ({ default: m.HomePage })));
const DashboardPage = lazy(() => import('pages/dashboard/DashboardPage').then((m) => ({ default: m.DashboardPage })));
const TokensPage = lazy(() => import('pages/tokens/TokensPage').then((m) => ({ default: m.TokensPage })));
const SwingDetectionPage = lazy(() => import('@analysis/pages/analysis/SwingDetectionPage').then((m) => ({ default: m.SwingDetectionPage })));
const OtherProfilesPage = lazy(() => import('pages/profiles/OtherProfilesPage').then((m) => ({ default: m.OtherProfilesPage })));
const Tpsl1Page = lazy(() => import('@analysis/pages/strategies/Tpsl1Page').then((m) => ({ default: m.Tpsl1Page })));
const Tpsl2Page = lazy(() => import('@analysis/pages/strategies/Tpsl2Page').then((m) => ({ default: m.Tpsl2Page })));
const Tpsl1GroupedSweepPage = lazy(() => import('@analysis/pages/strategies/sweep/Tpsl1GroupedSweepPage').then((m) => ({ default: m.Tpsl1GroupedSweepPage })));
const Tpsl2GroupedSweepPage = lazy(() => import('@analysis/pages/strategies/sweep/Tpsl2GroupedSweepPage').then((m) => ({ default: m.Tpsl2GroupedSweepPage })));
const SettingsPage = lazy(() => import('pages/settings/SettingsPage').then((m) => ({ default: m.SettingsPage })));
const NotFoundPage = lazy(() => import('pages/not-found/NotFoundPage').then((m) => ({ default: m.NotFoundPage })));

export default function App() {
  return (
    <BrowserRouter>
      <AppProviders>
        {/* Analysis-only: background-jobs SSE + status seed stays out of deploy. */}
        <BackgroundJobsProvider>
          <RouteErrorBoundary variant="root">
            <Suspense fallback={<SuspenseFallback />}>
              <Routes>
                <Route
                  element={<AppLayout nav={analysisNav} footer={<BackgroundJobsIndicator />} />}
                >
                  <Route index element={<HomePage />} />
                  <Route
                    path="dashboard"
                    element={
                      <DashboardPage
                        extraSections={({ tz, segment }) => (
                          <GroupedCreationSection tz={tz} segment={segment} />
                        )}
                      />
                    }
                  />
                  <Route path="tokens" element={<TokensPage />} />
                  <Route path="analysis" element={<Navigate to="/analysis/swing-detection" replace />} />
                  <Route path="analysis/swing-detection" element={<SwingDetectionPage />} />
                  <Route path="profiles/other" element={<OtherProfilesPage />} />
                  <Route path="strategies/tpsl1" element={<Tpsl1Page />} />
                  <Route path="strategies/tpsl2" element={<Tpsl2Page />} />
                  <Route path="strategies/grouped-sweep-tpsl1" element={<Tpsl1GroupedSweepPage />} />
                  <Route path="strategies/grouped-sweep-tpsl2" element={<Tpsl2GroupedSweepPage />} />
                  <Route path="strategies" element={<Navigate to="/strategies/tpsl2" replace />} />
                  <Route path="settings" element={<SettingsPage />} />
                  <Route path="*" element={<NotFoundPage />} />
                </Route>
              </Routes>
            </Suspense>
          </RouteErrorBoundary>
        </BackgroundJobsProvider>
      </AppProviders>
    </BrowserRouter>
  );
}
