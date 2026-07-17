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
const TraderAnalysisPage = lazy(() => import('@lab/pages/analysis/TraderAnalysisPage').then((m) => ({ default: m.TraderAnalysisPage })));
const ProfilesPage = lazy(() => import('pages/profiles/ProfilesPage').then((m) => ({ default: m.ProfilesPage })));
const RulesPage = lazy(() => import('@lab/pages/strategies/RulesPage').then((m) => ({ default: m.RulesPage })));
const FingerprintsPage = lazy(() => import('@lab/pages/strategies/FingerprintsPage').then((m) => ({ default: m.FingerprintsPage })));
const SimulatePage = lazy(() => import('@lab/pages/strategies/SimulatePage').then((m) => ({ default: m.SimulatePage })));
const MetricPanesPage = lazy(() => import('@lab/pages/strategies/MetricPanesPage').then((m) => ({ default: m.MetricPanesPage })));
const GenericSweepPage = lazy(() => import('@lab/pages/strategies/sweep/GenericSweepPage').then((m) => ({ default: m.GenericSweepPage })));
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
                  <Route path="profiles" element={<ProfilesPage />} />
                  <Route path="strategies/rules" element={<RulesPage />} />
                  <Route path="strategies/fingerprints" element={<FingerprintsPage />} />
                  <Route path="strategies/simulate" element={<SimulatePage />} />
                  <Route path="strategies/metric-panes" element={<MetricPanesPage />} />
                  <Route path="strategies/sweep" element={<GenericSweepPage />} />
                  <Route path="strategies" element={<Navigate to="/strategies/rules" replace />} />
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
