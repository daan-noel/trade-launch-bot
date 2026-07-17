import type { StatVariant } from 'utils/format';
import type { AddressKind } from 'utils/addressLinks';
import { AddressDisplay } from './AddressDisplay';
import { StatTile, type StatTone } from './StatTile';

/** Map legacy StatCard variants onto StatTile tones. */
function variantTone(variant: StatVariant | undefined): StatTone {
  switch (variant) {
    case 'primary':
    case 'accent':
      return 'primary';
    case 'info':
      return 'info';
    case 'muted':
      return 'muted';
    case 'danger':
      return 'red';
    case 'warning':
      return 'red';
    default:
      return 'default';
  }
}

interface StatCardProps {
  label: string;
  value: string;
  variant?: StatVariant;
  large?: boolean;
  bold?: boolean;
  href?: string;
}

/** Thin alias over {@link StatTile} — keeps TokenDetailPanel call sites stable. */
export function StatCard({
  label,
  value,
  variant = 'default',
  large,
  bold,
  href,
}: StatCardProps) {
  return (
    <StatTile
      label={label}
      value={value}
      tone={variantTone(variant)}
      size={large ? 'md' : 'sm'}
      bold={bold}
      href={href}
    />
  );
}

interface AddrCardProps {
  label: string;
  full: string;
  kind: AddressKind;
}

export function AddrCard({ label, full, kind }: AddrCardProps) {
  return (
    <div className="grid min-h-10 content-center gap-0.5 rounded-md border border-white/5 bg-white/2 px-2 py-1">
      <span className="truncate text-[10px] font-semibold uppercase tracking-wider text-text-dim">
        {label}
      </span>
      <AddressDisplay address={full} kind={kind} mode="full" />
    </div>
  );
}
