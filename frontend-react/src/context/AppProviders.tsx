import type { ReactNode } from 'react';
import { PriceUnitProvider } from './PriceUnitContext';
import { TimezoneProvider } from './TimezoneContext';

export function AppProviders({ children }: { children: ReactNode }) {
  return (
    <TimezoneProvider>
      <PriceUnitProvider>{children}</PriceUnitProvider>
    </TimezoneProvider>
  );
}
