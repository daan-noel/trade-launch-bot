import type { ReactNode } from 'react';

import { Badge, type BadgeSize } from 'components/ui/Badge';
import { cn } from 'lib/cn';
import { modeBadgeVariant } from 'lib/strategy/mode';
import type { TradeMode } from 'lib/strategy/types';

interface ModeBadgeProps {
  mode: TradeMode;
  size?: BadgeSize;
  /** Override label; defaults to the mode key (`paper` / `real`), uppercased. */
  children?: ReactNode;
  className?: string;
}

/** Compact trade-mode pill — always uses `modeBadgeVariant` (info/warning). */
export function ModeBadge({ mode, size = 'sm', children, className }: ModeBadgeProps) {
  return (
    <Badge
      variant={modeBadgeVariant(mode)}
      size={size}
      pill
      className={cn('uppercase', className)}
    >
      {children ?? mode}
    </Badge>
  );
}
