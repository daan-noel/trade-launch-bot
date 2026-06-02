import { BrowserRouter, Navigate, Route, Routes } from 'react-router-dom';
import { AppProviders } from './context/AppProviders';
import { AppLayout } from './components/layout/AppLayout';
import { HomePage } from './pages/HomePage';
import { DashboardPage } from './pages/DashboardPage';
import { TokensPage } from './pages/TokensPage';
import { TransactionsPage } from './pages/TransactionsPage';
import { AnalysisPage } from './pages/AnalysisPage';
import { WalletPage } from './pages/WalletPage';
import { TpslPage } from './pages/TpslPage';
import { SettingsPage } from './pages/SettingsPage';
import { NotFoundPage } from './pages/NotFoundPage';

export default function App() {
  return (
    <BrowserRouter>
      <AppProviders>
        <Routes>
          <Route element={<AppLayout />}>
            <Route index element={<HomePage />} />
            <Route path="dashboard" element={<DashboardPage />} />
            <Route path="tokens" element={<TokensPage />} />
            <Route path="transactions" element={<TransactionsPage />} />
            <Route path="analysis" element={<AnalysisPage />} />
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
