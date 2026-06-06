import { cn } from 'lib/cn';
import type { StatVariant } from 'utils/format';
import { statVariantClass } from 'utils/format';
import type { AddressKind } from 'utils/addressLinks';
import { AddressDisplay } from './AddressDisplay';

interface StatCardProps {
  label: string;
  value: string;
  variant?: StatVariant;
  large?: boolean;
  bold?: boolean;
  href?: string;
}

export function StatCard({
  label,
  value,
  variant = 'default',
  large,
  bold,
  href,
}: StatCardProps) {
  const valueCls = cn(
    'font-mono',
    statVariantClass(variant),
    large && 'text-base font-semibold',
    bold && 'font-semibold',
  );

  return (
    <div className="grid min-h-10 content-center gap-0.5 rounded-md border border-white/5 bg-white/2 px-1.5 py-1 transition hover:border-white/10">
      <span className="truncate text-[9px] font-semibold uppercase tracking-wider text-text-dim">
        {label}
      </span>
      {href ? (
        <a
          href={href}
          target="_blank"
          rel="noopener noreferrer"
          className={cn(valueCls, 'text-accent hover:text-primary hover:underline')}
        >
          {value}
        </a>
      ) : (
        <span className={valueCls}>{value}</span>
      )}
    </div>
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
      <span className="truncate text-[9px] font-semibold uppercase tracking-wider text-text-dim">
        {label}
      </span>
      <AddressDisplay
        address={full}
        kind={kind}
        mode="full"
      />
    </div>
  );
}
