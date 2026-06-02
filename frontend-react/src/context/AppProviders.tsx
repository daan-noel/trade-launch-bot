import type { ReactNode } from 'react';
import { PriceUnitProvider } from './PriceUnitContext';

export function AppProviders({ children }: { children: ReactNode }) {
  return <PriceUnitProvider>{children}</PriceUnitProvider>;
}
