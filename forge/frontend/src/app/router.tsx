import { createBrowserRouter } from 'react-router-dom';
import { AppShell } from './AppShell';
import { DashboardPage } from '@features/dashboard/DashboardPage';
import { LaunchConsolePage } from '@features/launch/LaunchConsolePage';
import { LaunchesPage } from '@features/launches/LaunchesPage';
import { TokenDetailPage } from '@features/launches/TokenDetailPage';
import { WalletPoolPage } from '@features/wallets/WalletPoolPage';
import { LaunchTemplatesPage } from '@features/templates/LaunchTemplatesPage';
import { MetadataTemplatesPage } from '@features/metadata/MetadataTemplatesPage';

export const router = createBrowserRouter([
  {
    path: '/',
    element: <AppShell />,
    children: [
      { index: true, element: <DashboardPage /> },
      { path: 'launch', element: <LaunchConsolePage /> },
      { path: 'launches', element: <LaunchesPage /> },
      { path: 'tokens/:mint', element: <TokenDetailPage /> },
      { path: 'wallets', element: <WalletPoolPage /> },
      { path: 'templates', element: <LaunchTemplatesPage /> },
      { path: 'metadata', element: <MetadataTemplatesPage /> },
    ],
  },
]);
