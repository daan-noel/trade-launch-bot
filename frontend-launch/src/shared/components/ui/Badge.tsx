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

// Single source of truth for wallet-role → color. Each known role gets a distinct
// hue (see --color-role-* in index.css); unknown roles fall back to muted.
const KNOWN_ROLES = new Set(['dev', 'bundler', 'treasury', 'trading']);

/** CSS var holding a role's accent color — for tinting summary cards, bars, etc. */
export function roleColorVar(role: string): string {
  const r = role.toLowerCase();
  return KNOWN_ROLES.has(r) ? `var(--color-role-${r})` : 'var(--color-muted)';
}

export function RolePill({ role }: { role: string | null | undefined }) {
  if (!role) return <span className="muted">—</span>;
  const r = role.toLowerCase();
  const key = KNOWN_ROLES.has(r) ? r : 'unknown';
  return <span className={clsx('badge', `badge-role-${key}`)}>{role}</span>;
}
