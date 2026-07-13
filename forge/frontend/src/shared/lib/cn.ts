import clsx, { type ClassValue } from 'clsx';

/** Conditional classname join — thin alias over `clsx` so ported components that
 *  call `cn(...)` work unchanged. */
export function cn(...inputs: ClassValue[]): string {
  return clsx(inputs);
}
