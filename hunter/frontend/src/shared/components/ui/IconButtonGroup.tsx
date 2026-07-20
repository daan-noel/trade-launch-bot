import type { HTMLAttributes, ReactNode } from 'react';
import { cn } from 'lib/cn';

/**
 * Centers a cluster of IconButtons in their cell/toolbar context
 * (`inline-flex` so a `text-center` parent can center the group).
 */
export function IconButtonGroup({
  className,
  children,
  ...props
}: HTMLAttributes<HTMLDivElement> & { children: ReactNode }) {
  return (
    <div
      role="group"
      className={cn('inline-flex items-center justify-center gap-1', className)}
      {...props}
    >
      {children}
    </div>
  );
}
