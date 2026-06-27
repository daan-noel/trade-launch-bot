import type { ReactNode } from 'react';
import { PriceUnitProvider } from './PriceUnitContext';
import { TimezoneProvider } from './TimezoneContext';
import { ToastProvider } from 'components/ui/Toast';

/**
 * Shared root providers — timezone, price-unit, and toast — wrapped by both
 * builds. The analysis-only `BackgroundJobsProvider` is NOT here; the analysis
 * `App` nests it itself so its analysis-only SSE wiring stays out of the deploy
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
