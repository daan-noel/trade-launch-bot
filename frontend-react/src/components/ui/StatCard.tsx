import { cn } from '../../lib/cn';
import type { StatVariant } from '../../utils/format';
import { statVariantClass } from '../../utils/format';

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
  short: string;
  full: string;
  solscanUrl: string;
  gmgnUrl?: string;
}

export function AddrCard({ label, short, full, solscanUrl, gmgnUrl }: AddrCardProps) {
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(full);
    } catch {
      /* ignore */
    }
  };

  return (
    <div className="grid min-h-10 content-center gap-0.5 rounded-md border border-white/5 bg-white/2 px-2 py-1">
      <span className="truncate text-[9px] font-semibold uppercase tracking-wider text-text-dim">
        {label}
      </span>
      <div className="flex items-center justify-between gap-1">
        <button
          type="button"
          onClick={copy}
          title="Click to copy"
          className="min-w-0 flex-1 truncate text-left font-mono text-[10px] text-text-mid hover:text-text"
        >
          {short}
        </button>
        <div className="flex shrink-0 items-center gap-0.5">
          {gmgnUrl && (
            <a
              href={gmgnUrl}
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex size-[18px] items-center justify-center rounded bg-white/5 text-[9px] font-bold text-[#00c97a] hover:bg-[rgba(0,201,122,0.15)]"
              title="Open on GMGN"
            >
              G
            </a>
          )}
          <a
            href={solscanUrl}
            target="_blank"
            rel="noopener noreferrer"
            className="inline-flex size-[18px] items-center justify-center rounded bg-white/5 text-[9px] font-bold text-[#9945ff] hover:bg-[rgba(153,69,255,0.15)]"
            title="Open on Solscan"
          >
            S
          </a>
        </div>
      </div>
    </div>
  );
}
