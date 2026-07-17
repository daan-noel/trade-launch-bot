import { lazy, Suspense } from 'react';
import { BrowserRouter, Navigate, Route, Routes } from 'react-router-dom';
import { AppProviders } from 'context/AppProviders';
import { AppLayout } from 'components/layout/AppLayout';
import { RouteErrorBoundary } from 'components/ui/ErrorBoundary';
import { SuspenseFallback } from 'components/ui/SuspenseFallback';
import { BackgroundJobsProvider } from '@lab/context/BackgroundJobsContext';
import { BackgroundJobsIndicator } from '@lab/components/layout/BackgroundJobsIndicator';
import { GroupedCreationSection } from '@lab/components/creation-stats/GroupedCreationSection';
import { labNav } from './nav';

// Code-split each route into its own chunk. Pages export named (not default)
// components, so map the named export onto `default` in each lazy() call.
const HomePage = lazy(() => import('pages/home/HomePage').then((m) => ({ default: m.HomePage })));
const CreationStatsPage = lazy(() => import('@lab/pages/creation-stats/CreationStatsPage').then((m) => ({ default: m.CreationStatsPage })));
const TokensPage = lazy(() => import('pages/tokens/TokensPage').then((m) => ({ default: m.TokensPage })));
const SwingDetectionPage = lazy(() => import('@lab/pages/analysis/SwingDetectionPage').then((m) => ({ default: m.SwingDetectionPage })));
const Swing1DetectPage = lazy(() => import('@lab/pages/analysis/Swing1DetectPage').then((m) => ({ default: m.Swing1DetectPage })));
const TraderAnalysisPage = lazy(() => import('@lab/pages/analysis/TraderAnalysisPage').then((m) => ({ default: m.TraderAnalysisPage })));
const ProfilesPage = lazy(() => import('pages/profiles/ProfilesPage').then((m) => ({ default: m.ProfilesPage })));
const Tpsl1Page = lazy(() => import('@lab/pages/strategies/Tpsl1Page').then((m) => ({ default: m.Tpsl1Page })));
const Tpsl2Page = lazy(() => import('@lab/pages/strategies/Tpsl2Page').then((m) => ({ default: m.Tpsl2Page })));
const Swing1Page = lazy(() => import('@lab/pages/strategies/Swing1Page').then((m) => ({ default: m.Swing1Page })));
const RulesPage = lazy(() => import('@lab/pages/strategies/RulesPage').then((m) => ({ default: m.RulesPage })));
const FingerprintsPage = lazy(() => import('@lab/pages/strategies/FingerprintsPage').then((m) => ({ default: m.FingerprintsPage })));
const SimulatePage = lazy(() => import('@lab/pages/strategies/SimulatePage').then((m) => ({ default: m.SimulatePage })));
const Tpsl1GroupedSweepPage = lazy(() => import('@lab/pages/strategies/sweep/Tpsl1GroupedSweepPage').then((m) => ({ default: m.Tpsl1GroupedSweepPage })));
const Tpsl2GroupedSweepPage = lazy(() => import('@lab/pages/strategies/sweep/Tpsl2GroupedSweepPage').then((m) => ({ default: m.Tpsl2GroupedSweepPage })));
const Swing1GroupedSweepPage = lazy(() => import('@lab/pages/strategies/sweep/Swing1GroupedSweepPage').then((m) => ({ default: m.Swing1GroupedSweepPage })));
const SettingsPage = lazy(() => import('pages/settings/SettingsPage').then((m) => ({ default: m.SettingsPage })));
const NotFoundPage = lazy(() => import('pages/not-found/NotFoundPage').then((m) => ({ default: m.NotFoundPage })));

export default function App() {
  return (
    <BrowserRouter>
      <AppProviders>
        {/* Lab-only: background-jobs SSE + status seed stays out of the live build. */}
        <BackgroundJobsProvider>
          <RouteErrorBoundary variant="root">
            <Suspense fallback={<SuspenseFallback />}>
              <Routes>
                <Route
                  element={<AppLayout nav={labNav} footer={<BackgroundJobsIndicator />} />}
                >
                  <Route index element={<HomePage />} />
                  <Route
                    path="creation-stats"
                    element={
                      <CreationStatsPage
                        extraSections={({ tz, segment }) => (
                          <GroupedCreationSection tz={tz} segment={segment} />
                        )}
                      />
                    }
                  />
                  <Route path="tokens" element={<TokensPage />} />
                  <Route path="analysis" element={<Navigate to="/analysis/trader" replace />} />
                  <Route path="analysis/trader" element={<TraderAnalysisPage />} />
                  <Route path="analysis/swing-detection" element={<SwingDetectionPage />} />
                  <Route path="analysis/swing1-detect" element={<Swing1DetectPage />} />
                  <Route path="profiles" element={<ProfilesPage />} />
                  <Route path="strategies/rules" element={<RulesPage />} />
                  <Route path="strategies/fingerprints" element={<FingerprintsPage />} />
                  <Route path="strategies/simulate" element={<SimulatePage />} />
                  <Route path="strategies/tpsl1" element={<Tpsl1Page />} />
                  <Route path="strategies/tpsl2" element={<Tpsl2Page />} />
                  <Route path="strategies/swing1" element={<Swing1Page />} />
                  <Route path="strategies/grouped-sweep-tpsl1" element={<Tpsl1GroupedSweepPage />} />
                  <Route path="strategies/grouped-sweep-tpsl2" element={<Tpsl2GroupedSweepPage />} />
                  <Route path="strategies/grouped-sweep-swing1" element={<Swing1GroupedSweepPage />} />
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
