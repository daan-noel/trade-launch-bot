import type { ReactNode } from 'react';
import { PriceUnitProvider } from './PriceUnitContext';
import { TimezoneProvider } from './TimezoneContext';
import { BackgroundJobsProvider } from './BackgroundJobsContext';

export function AppProviders({ children }: { children: ReactNode }) {
  return (
    <TimezoneProvider>
      <PriceUnitProvider>
        <BackgroundJobsProvider>{children}</BackgroundJobsProvider>
      </PriceUnitProvider>
    </TimezoneProvider>
  );
}
