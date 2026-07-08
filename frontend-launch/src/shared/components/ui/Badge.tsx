import { ReactNode } from 'react';
import clsx from 'clsx';

export type Tone = 'good' | 'warn' | 'bad' | 'neutral' | 'info';

const TONE_CLASS: Record<Tone, string> = {
  good: 'badge-good',
  warn: 'badge-warn',
  bad: 'badge-bad',
  neutral: 'badge-neutral',
  info: 'badge-info',
};

export function Badge({ tone = 'neutral', children }: { tone?: Tone; children: ReactNode }) {
  return <span className={clsx('badge', TONE_CLASS[tone])}>{children}</span>;
}

// Single source of truth for status → color across launches, bundles, wallets.
// Normalizes casing/separators so "in-progress" and "in_progress" agree.
const STATUS_TONE: Record<string, Tone> = {
  // good / terminal-success
  created: 'good',
  landed: 'good',
  funded: 'good',
  confirmed: 'good',
  // warn / in-flight
  pending: 'warn',
  planned: 'warn',
  submitting: 'warn',
  submitted: 'warn',
  funding: 'warn',
  reserved: 'warn',
  // bad / terminal-failure
  failed: 'bad',
  dropped: 'bad',
  partial: 'bad',
  retired: 'bad',
  dead: 'bad',
  // neutral
  generated: 'neutral',
  used: 'neutral',
};

export function statusTone(status: string): Tone {
  return STATUS_TONE[status.toLowerCase().replace(/[^a-z]/g, '')] ?? 'neutral';
}

export function StatusPill({ status }: { status: string | null | undefined }) {
  if (!status) return <span className="muted">—</span>;
  return <Badge tone={statusTone(status)}>{status}</Badge>;
}
