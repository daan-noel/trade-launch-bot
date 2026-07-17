// Compact at-a-glance chip cluster for a fingerprint's match axes. Shared by
// Rules, Simulate, and the rule-editor picker — one SSOT so every surface that
// shows a fingerprint reads the same. Null / empty axes are omitted; bucket is
// always shown (every fingerprint has a width).

import type { ReactNode } from 'react';
import { cn } from 'lib/cn';
import { formatCompact, formatDecimalTrim } from 'utils/format';
import { formatIxLabelsText } from 'lib/ixLabels';
import { lamportsToSol, type Fingerprint } from 'lib/strategy/types';

function chip(text: ReactNode, opts?: { cls?: string; title?: string }): ReactNode {
  return (
    <span
      title={opts?.title}
      className={cn(
        'inline-block rounded border border-white/10 bg-surface px-1.5 py-0.5 font-mono text-[11px] leading-tight',
        opts?.cls,
      )}
    >
      {text}
    </span>
  );
}

function solChip(label: string, lamports: number | null): ReactNode | null {
  const s = lamportsToSol(lamports);
  if (s == null) return null;
  return chip(`${label}=${formatDecimalTrim(s, 4)}◎`);
}

function intChip(label: string, n: number | null): ReactNode | null {
  if (n == null) return null;
  return chip(`${label}=${formatCompact(n, 1)}`);
}

/** Axis chips only (no name) — set criteria + always-on bucket width. */
export function fingerprintParamsCell(fp: Fingerprint): ReactNode {
  const ix = fp.ix_labels?.length ? fp.ix_labels : null;
  const chips: ReactNode[] = [
    intChip('cu_limit', fp.cu_limit),
    intChip('cu_price', fp.cu_price),
    solChip('init', fp.init_buy_lamports),
    solChip('max', fp.max_cost_lamports),
    solChip('spend', fp.spendable_lamports_in),
    solChip('fs_buy', fp.first_slot_buy_lamports),
    solChip('fs_sell', fp.first_slot_sell_lamports),
    ix
      ? chip(`${ix.length}ix`, { title: formatIxLabelsText(ix) })
      : null,
    chip(`bkt=${formatDecimalTrim(fp.bucket_size_amount, 4)}◎`, { cls: 'text-text-dim' }),
  ].filter(Boolean);

  return <div className="flex flex-wrap items-center gap-1 text-left">{chips}</div>;
}

/** Flat searchable text for table filters (axis labels + values + name). */
export function fingerprintParamsSearchText(fp: Fingerprint | undefined, fallbackId?: string): string {
  if (!fp) return fallbackId ?? '';
  const parts: string[] = [fp.name || fp.id.slice(0, 8)];
  if (fp.cu_limit != null) parts.push(`cu_limit=${fp.cu_limit}`);
  if (fp.cu_price != null) parts.push(`cu_price=${fp.cu_price}`);
  const pushSol = (label: string, lamports: number | null) => {
    const s = lamportsToSol(lamports);
    if (s != null) parts.push(`${label}=${formatDecimalTrim(s, 4)}`);
  };
  pushSol('init', fp.init_buy_lamports);
  pushSol('max', fp.max_cost_lamports);
  pushSol('spend', fp.spendable_lamports_in);
  pushSol('fs_buy', fp.first_slot_buy_lamports);
  pushSol('fs_sell', fp.first_slot_sell_lamports);
  if (fp.ix_labels?.length) {
    parts.push(`${fp.ix_labels.length}ix`);
    parts.push(formatIxLabelsText(fp.ix_labels));
  }
  parts.push(`bkt=${formatDecimalTrim(fp.bucket_size_amount, 4)}`);
  return parts.join(' ');
}
