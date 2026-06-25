import type { ReactNode } from 'react';
import { PriceUnitProvider } from './PriceUnitContext';
import { TimezoneProvider } from './TimezoneContext';
import { BackgroundJobsProvider } from './BackgroundJobsContext';
import { ToastProvider } from 'components/ui/Toast';

export function AppProviders({ children }: { children: ReactNode }) {
  return (
    <TimezoneProvider>
      <PriceUnitProvider>
        <BackgroundJobsProvider>
          <ToastProvider>{children}</ToastProvider>
        </BackgroundJobsProvider>
      </PriceUnitProvider>
    </TimezoneProvider>
  );
}
