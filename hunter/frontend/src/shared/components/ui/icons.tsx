import type { ReactNode } from 'react';
import { cn } from 'lib/cn';

/** Shared stroke icons for icon-only action buttons (20×20 viewBox). */
type IconProps = { className?: string };

function Svg({ className, children }: IconProps & { children: ReactNode }) {
  return (
    <svg
      viewBox="0 0 20 20"
      fill="none"
      aria-hidden
      // `block` avoids SVG baseline gap when sitting next to text
      className={cn('block size-4', className)}
    >
      {children}
    </svg>
  );
}

const stroke = {
  stroke: 'currentColor',
  strokeWidth: 1.6,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
};

export function PlayIcon({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M6.5 4.5v11l9-5.5-9-5.5Z" fill="currentColor" stroke="none" />
    </Svg>
  );
}

/** Lab backtest / simulate — flask, distinct from live Activate (PlayIcon). */
export function SimulateIcon({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path
        d="M8 3.5h4M9 3.5v4.2L5.2 14.2A2.2 2.2 0 0 0 7.1 17.5h5.8a2.2 2.2 0 0 0 1.9-3.3L11 7.7V3.5"
        {...stroke}
      />
      <path d="M6.8 13.5h6.4" {...stroke} />
    </Svg>
  );
}

export function SaveIcon({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M4 3.5h9.5L16.5 6.5V16.5H4V3.5Z" {...stroke} />
      <path d="M7 3.5v4h5.5v-4M7 16.5v-5h6v5" {...stroke} />
    </Svg>
  );
}

export function TrashIcon({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M4.5 6.5h11M8 6.5V4.5h4v2M6.5 6.5l.7 9h5.6l.7-9" {...stroke} />
    </Svg>
  );
}

export function PlusIcon({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M10 4.5v11M4.5 10h11" {...stroke} />
    </Svg>
  );
}

export function SearchIcon({ className }: IconProps) {
  return (
    <Svg className={className}>
      <circle cx="8.5" cy="8.5" r="4.5" {...stroke} />
      <path d="M12 12l4 4" {...stroke} />
    </Svg>
  );
}

export function PromoteIcon({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M10 15.5V5.5M6.5 9l3.5-3.5L13.5 9M5 15.5h10" {...stroke} />
    </Svg>
  );
}

export function RefreshIcon({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M15.5 8.5A5.5 5.5 0 1 0 14 14.2" {...stroke} />
      <path d="M15.5 4.5v4h-4" {...stroke} />
    </Svg>
  );
}

export function EditIcon({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M12.5 4.5 15.5 7.5 7.5 15.5H4.5v-3L12.5 4.5Z" {...stroke} />
    </Svg>
  );
}

export function DuplicateIcon({ className }: IconProps) {
  return (
    <Svg className={className}>
      <rect x="6.5" y="6.5" width="9" height="9" rx="1" {...stroke} />
      <path d="M13.5 6.5V4.5a1 1 0 0 0-1-1h-8a1 1 0 0 0-1 1v8a1 1 0 0 0 1 1h2" {...stroke} />
    </Svg>
  );
}

export function PauseIcon({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M6.5 4.5h2.5v11H6.5zM11 4.5h2.5v11H11z" fill="currentColor" stroke="none" />
    </Svg>
  );
}

export function StopIcon({ className }: IconProps) {
  return (
    <Svg className={className}>
      <rect x="5.5" y="5.5" width="9" height="9" rx="1" fill="currentColor" stroke="none" />
    </Svg>
  );
}

export function CheckIcon({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M4.5 10.5 8 14l7.5-8" {...stroke} />
    </Svg>
  );
}

/** Power-on — enable a soft-archived rule. */
export function EnableIcon({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M10 3.5v6" {...stroke} />
      <path d="M6.2 5.8a5.5 5.5 0 1 0 7.6 0" {...stroke} />
    </Svg>
  );
}

/** Power-off — soft-archive (disable) a rule without deleting it. */
export function DisableIcon({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M10 3.5v6" {...stroke} />
      <path d="M6.2 5.8a5.5 5.5 0 1 0 7.6 0" {...stroke} />
      <path d="M4.5 4.5 15.5 15.5" {...stroke} />
    </Svg>
  );
}

export function ReuseIcon({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M4.5 8.5A5.5 5.5 0 0 1 14 5.8" {...stroke} />
      <path d="M4.5 11.5v-3h3M15.5 11.5A5.5 5.5 0 0 1 6 14.2" {...stroke} />
      <path d="M15.5 8.5v3h-3" {...stroke} />
    </Svg>
  );
}

export function CloseIcon({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M5.5 5.5 14.5 14.5M14.5 5.5 5.5 14.5" {...stroke} />
    </Svg>
  );
}

export function BuyIcon({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M10 4.5v8M6.5 9.5 10 13l3.5-3.5M4.5 15.5h11" {...stroke} />
    </Svg>
  );
}

export function SellIcon({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M10 15.5v-8M6.5 10.5 10 7l3.5 3.5M4.5 4.5h11" {...stroke} />
    </Svg>
  );
}

export function LinkIcon({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path
        d="M8.5 11.5a3 3 0 0 1 0-4.2l1.8-1.8a3 3 0 0 1 4.2 4.2l-.9.9M11.5 8.5a3 3 0 0 1 0 4.2l-1.8 1.8a3 3 0 1 1-4.2-4.2l.9-.9"
        {...stroke}
      />
    </Svg>
  );
}

export function SettingsIcon({ className }: IconProps) {
  return (
    <Svg className={className}>
      <circle cx="10" cy="10" r="2.2" {...stroke} />
      <path
        d="M10 3.5v1.8M10 14.7v1.8M3.5 10h1.8M14.7 10h1.8M5.4 5.4l1.3 1.3M13.3 13.3l1.3 1.3M14.6 5.4l-1.3 1.3M6.7 13.3l-1.3 1.3"
        {...stroke}
      />
    </Svg>
  );
}

/** Closed padlock — field is locked (click to unlock). */
export function LockIcon({ className }: IconProps) {
  return (
    <Svg className={className}>
      <rect x="5.5" y="9" width="9" height="7" rx="1.2" {...stroke} />
      <path d="M7.5 9V7a2.5 2.5 0 0 1 5 0v2" {...stroke} />
    </Svg>
  );
}

/** Open padlock — field is unlocked (click to re-lock). */
export function UnlockIcon({ className }: IconProps) {
  return (
    <Svg className={className}>
      <rect x="5.5" y="9" width="9" height="7" rx="1.2" {...stroke} />
      <path d="M7.5 9V7a2.5 2.5 0 0 1 5 0" {...stroke} />
    </Svg>
  );
}

/** Lightweight spinner for loading icon-button states. */
export function SpinnerIcon({ className }: IconProps) {
  return (
    <svg
      viewBox="0 0 20 20"
      fill="none"
      aria-hidden
      className={cn('block size-4 animate-spin', className)}
    >
      <circle
        cx="10"
        cy="10"
        r="6.5"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeOpacity="0.25"
      />
      <path
        d="M16.5 10a6.5 6.5 0 0 0-6.5-6.5"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinecap="round"
      />
    </svg>
  );
}
