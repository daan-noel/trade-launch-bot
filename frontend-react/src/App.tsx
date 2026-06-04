import { BrowserRouter, Navigate, Route, Routes } from 'react-router-dom';
import { AppProviders } from './context/AppProviders';
import { AppLayout } from './components/layout/AppLayout';
import { HomePage } from './pages/home/HomePage';
import { DashboardPage } from './pages/dashboard/DashboardPage';
import { TokensPage } from './pages/tokens/TokensPage';
import { SyncTokenPage } from './pages/tokens/SyncTokenPage';
import { TransactionsPage } from './pages/transactions/TransactionsPage';
import { AnalysisPage } from './pages/analysis/AnalysisPage';
import { SwingDetectionPage } from './pages/analysis/SwingDetectionPage';
import { WalletPage } from './pages/wallet/WalletPage';
import { TpslPage } from './pages/strategies/TpslPage';
import { SettingsPage } from './pages/settings/SettingsPage';
import { NotFoundPage } from './pages/not-found/NotFoundPage';

export default function App() {
  return (
    <BrowserRouter>
      <AppProviders>
        <Routes>
          <Route element={<AppLayout />}>
            <Route index element={<HomePage />} />
            <Route path="dashboard" element={<DashboardPage />} />
            <Route path="tokens" element={<TokensPage />} />
            <Route path="token/sync" element={<SyncTokenPage />} />
            <Route path="transactions" element={<TransactionsPage />} />
            <Route path="analysis" element={<Navigate to="/analysis/general" replace />} />
            <Route path="analysis/general" element={<AnalysisPage />} />
            <Route path="analysis/swing-detection" element={<SwingDetectionPage />} />
            <Route path="wallet" element={<WalletPage />} />
            <Route path="strategies/tpsl" element={<TpslPage />} />
            <Route path="strategies" element={<Navigate to="/strategies/tpsl" replace />} />
            <Route path="settings" element={<SettingsPage />} />
            <Route path="*" element={<NotFoundPage />} />
          </Route>
        </Routes>
      </AppProviders>
    </BrowserRouter>
  );
}
