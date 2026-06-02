import { Outlet } from 'react-router-dom';
import { Header } from './Header';

export function AppLayout() {
  return (
    <div className="flex min-h-screen flex-col bg-bg text-text">
      <Header />
      <main className="flex-1 px-6 py-5 animate-[fade-in-up_0.25s_ease_both]">
        <Outlet />
      </main>
    </div>
  );
}
