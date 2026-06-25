import { Outlet } from 'react-router-dom';
import { Header } from './Header';
import { BackgroundJobsIndicator } from './BackgroundJobsIndicator';
import { RouteErrorBoundary } from 'components/ui/ErrorBoundary';
import { usePositionNotifications } from 'hooks/usePositionNotifications';

function NotificationMount() {
  usePositionNotifications();
  return null;
}

export function AppLayout() {
  return (
    <div className="flex min-h-screen flex-col bg-bg text-text">
      <NotificationMount />
      <Header />
      <main className="flex-1 px-6 py-5 animate-[fade-in-up_0.25s_ease_both]">
        {/* A page-level throw degrades to a fallback card; the nav above stays live. */}
        <RouteErrorBoundary variant="page">
          <Outlet />
        </RouteErrorBoundary>
      </main>
      <BackgroundJobsIndicator />
    </div>
  );
}
