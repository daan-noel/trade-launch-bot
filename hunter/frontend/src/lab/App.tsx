import { lazy } from 'react';
import { BrowserRouter, Navigate, Route, Routes, useLocation } from 'react-router-dom';
import { AppProviders } from 'context/AppProviders';
import { AppLayout } from 'components/layout/AppLayout';
import { RouteErrorBoundary } from 'components/ui/ErrorBoundary';
import { BackgroundJobsProvider } from '@lab/context/BackgroundJobsContext';
import { BackgroundJobsIndicator } from '@lab/components/layout/BackgroundJobsIndicator';
import { labNav } from './nav';

// Code-split each route into its own chunk. Pages export named (not default)
// components, so map the named export onto `default` in each lazy() call.
const HomePage = lazy(() => import('@lab/pages/home/LabHomePage').then((m) => ({ default: m.LabHomePage })));
const CreationStatsPage = lazy(() => import('@lab/pages/creation-stats/CreationStatsPage').then((m) => ({ default: m.CreationStatsPage })));
const TokensPage = lazy(() => import('@lab/pages/tokens/LabTokensPage').then((m) => ({ default: m.LabTokensPage })));
const TraderAnalysisPage = lazy(() => import('@lab/pages/analysis/TraderAnalysisPage').then((m) => ({ default: m.TraderAnalysisPage })));
const ProfilesPage = lazy(() => import('pages/profiles/ProfilesPage').then((m) => ({ default: m.ProfilesPage })));
const RulesPage = lazy(() => import('@lab/pages/strategies/RulesPage').then((m) => ({ default: m.RulesPage })));
const FingerprintsPage = lazy(() => import('@lab/pages/strategies/FingerprintsPage').then((m) => ({ default: m.FingerprintsPage })));
const FlowDiscoveryPage = lazy(() => import('@lab/pages/strategies/FlowDiscoveryPage').then((m) => ({ default: m.FlowDiscoveryPage })));
const MetricDiscoveryPage = lazy(() => import('@lab/pages/strategies/MetricDiscoveryPage').then((m) => ({ default: m.MetricDiscoveryPage })));
const SimulatePage = lazy(() => import('@lab/pages/strategies/SimulatePage').then((m) => ({ default: m.SimulatePage })));
const GenericSweepPage = lazy(() => import('@lab/pages/strategies/sweep/GenericSweepPage').then((m) => ({ default: m.GenericSweepPage })));
const ReplayViewerPage = lazy(() => import('@lab/pages/strategies/ReplayViewerPage').then((m) => ({ default: m.ReplayViewerPage })));
const SettingsPage = lazy(() => import('pages/settings/SettingsPage').then((m) => ({ default: m.SettingsPage })));
const NotFoundPage = lazy(() => import('pages/not-found/NotFoundPage').then((m) => ({ default: m.NotFoundPage })));

/** Metric panes folded into Tokens — keep old URLs working. */
function MetricPanesRedirect() {
  const { search } = useLocation();
  const params = new URLSearchParams(search);
  const mint = params.get('mint');
  const to = mint ? `/tokens?mint=${encodeURIComponent(mint)}` : '/tokens';
  return <Navigate to={to} replace />;
}

export default function App() {
  return (
    <BrowserRouter>
      <AppProviders>
        {/* Lab-only: background-jobs SSE + status seed stays out of the live build. */}
        <BackgroundJobsProvider>
          <RouteErrorBoundary variant="root">
            <Routes>
              <Route
                element={<AppLayout nav={labNav} footer={<BackgroundJobsIndicator />} />}
              >
                <Route index element={<HomePage />} />
                <Route path="creation-stats" element={<CreationStatsPage />} />
                <Route path="tokens" element={<TokensPage />} />
                <Route path="analysis" element={<Navigate to="/analysis/trader" replace />} />
                <Route path="analysis/trader" element={<TraderAnalysisPage />} />
                <Route path="profiles" element={<ProfilesPage />} />
                <Route path="strategies/rules" element={<RulesPage />} />
                <Route path="strategies/fingerprints" element={<FingerprintsPage />} />
                <Route path="strategies/flow-discovery" element={<FlowDiscoveryPage />} />
                <Route path="strategies/metric-discovery" element={<MetricDiscoveryPage />} />
                <Route path="strategies/simulate" element={<SimulatePage />} />
                <Route path="strategies/metric-panes" element={<MetricPanesRedirect />} />
                <Route path="strategies/sweep" element={<GenericSweepPage />} />
                <Route path="strategies/replay" element={<ReplayViewerPage />} />
                <Route path="strategies" element={<Navigate to="/strategies/rules" replace />} />
                <Route path="settings" element={<SettingsPage />} />
                <Route path="*" element={<NotFoundPage />} />
              </Route>
            </Routes>
          </RouteErrorBoundary>
        </BackgroundJobsProvider>
      </AppProviders>
    </BrowserRouter>
  );
}
