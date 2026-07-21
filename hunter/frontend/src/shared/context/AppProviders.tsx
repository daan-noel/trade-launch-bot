import type { ReactNode } from 'react';
import { PriceUnitProvider } from './PriceUnitProvider';
import { TimezoneProvider } from './TimezoneContext';
import { ToastProvider } from 'components/ui/Toast';

/**
 * Shared root providers — timezone, price-unit, and toast — wrapped by both
 * builds. The lab-only `BackgroundJobsProvider` is NOT here; the analysis
 * `App` nests it itself so its lab-only SSE wiring stays out of the live
 * bundle.
 */
export function AppProviders({ children }: { children: ReactNode }) {
  return (
    <TimezoneProvider>
      <PriceUnitProvider>
        <ToastProvider>{children}</ToastProvider>
      </PriceUnitProvider>
    </TimezoneProvider>
  );
}
