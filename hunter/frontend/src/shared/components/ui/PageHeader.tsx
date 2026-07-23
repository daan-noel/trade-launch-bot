import type { ReactNode } from 'react';
import { cn } from 'lib/cn';

type PageHeaderSize = 'page' | 'tool';

interface PageHeaderProps {
  title: ReactNode;
  /** One-line job description — what this surface is for. */
  description?: ReactNode;
  /** Primary / secondary actions (buttons) aligned to the right. */
  actions?: ReactNode;
  /**
   * `page` — homes / command centers (`text-2xl`).
   * `tool` — dense strategy surfaces (`text-lg`) — default.
   */
  size?: PageHeaderSize;
  className?: string;
}

const titleClass: Record<PageHeaderSize, string> = {
  page: 'text-2xl font-extrabold text-text',
  tool: 'text-lg font-extrabold text-text',
};

/**
 * Shared page chrome — title + job line + optional CTA cluster.
 * Prefer this over ad-hoc `h1` rows so hierarchy stays consistent across apps.
 */
export function PageHeader({
  title,
  description,
  actions,
  size = 'tool',
  className,
}: PageHeaderProps) {
  return (
    <div
      className={cn(
        'mb-4 flex flex-wrap items-start justify-between gap-3',
        className,
      )}
    >
      <div className="flex min-w-0 flex-wrap items-baseline gap-3">
        <h1 className={titleClass[size]}>{title}</h1>
        {description != null && (
          <span className="text-sm text-text-mid">{description}</span>
        )}
      </div>
      {actions != null && (
        <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div>
      )}
    </div>
  );
}
