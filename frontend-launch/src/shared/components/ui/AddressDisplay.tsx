import { useState } from 'react';
import clsx from 'clsx';
import { shortAddr, solscanAddr, solscanMint, solscanTx } from '@shared/lib/format';

type Kind = 'account' | 'token' | 'tx';

const LINK: Record<Kind, (v: string) => string> = {
  account: solscanAddr,
  token: solscanMint,
  tx: solscanTx,
};

/** Truncated mono address with a copy button and a Solscan link. */
export function AddressDisplay({
  value,
  kind = 'account',
  lead = 4,
  tail = 4,
  className,
  link = true,
}: {
  value: string | null | undefined;
  kind?: Kind;
  lead?: number;
  tail?: number;
  className?: string;
  link?: boolean;
}) {
  const [copied, setCopied] = useState(false);
  if (!value) return <span className="muted">—</span>;

  const onCopy = async (e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    } catch {
      /* clipboard unavailable — ignore */
    }
  };

  const label = shortAddr(value, lead, tail);
  return (
    <span className={clsx('inline-flex items-center gap-1.5 mono text-xs', className)}>
      {link ? (
        <a href={LINK[kind](value)} target="_blank" rel="noreferrer" onClick={(e) => e.stopPropagation()}>
          {label}
        </a>
      ) : (
        <span>{label}</span>
      )}
      <button
        type="button"
        onClick={onCopy}
        title="Copy"
        className="muted hover:text-[var(--color-ink)] transition-colors"
      >
        {copied ? '✓' : '⧉'}
      </button>
    </span>
  );
}
