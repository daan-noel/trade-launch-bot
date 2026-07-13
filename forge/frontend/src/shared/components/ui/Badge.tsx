import { ReactNode } from 'react';
import clsx from 'clsx';
import type { WalletRole } from '../../types';

export type Tone = 'good' | 'warn' | 'bad' | 'neutral' | 'info';

const TONE_CLASS: Record<Tone, string> = {
  good: 'badge-good',
  warn: 'badge-warn',
  bad: 'badge-bad',
  neutral: 'badge-neutral',
  info: 'badge-info',
};

// Single source of truth for tone → accent color var — the SAME tokens the .badge-*
// classes paint with (index.css), so JS-driven accents (status-bar segments, dots,
// PnL, etc.) can never drift from the pills. Anything outside a pill that needs a
// tone's color must read it from here, not re-list `var(--color-*)`.
const TONE_COLOR_VAR: Record<Tone, string> = {
  good: 'var(--color-good)',
  warn: 'var(--color-warn)',
  bad: 'var(--color-bad)',
  neutral: 'var(--color-muted)',
  info: 'var(--color-info)',
};

/** CSS var holding a tone's accent color — for tinting bars, dots, PnL, etc. */
export function toneColorVar(tone: Tone): string {
  return TONE_COLOR_VAR[tone];
}

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

// Single source of truth for trade/action type → color: a green buy vs a red sell
// read at a glance (consolidate = a neutral SOL sweep). Distinct from statusTone so
// buy/sell don't both fall through to a same-colored `neutral` pill.
const TRADE_TYPE_TONE: Record<string, Tone> = {
  buy: 'good',
  sell: 'bad',
  consolidate: 'info',
};

export function tradeTypeTone(type: string): Tone {
  return TRADE_TYPE_TONE[type.toLowerCase()] ?? 'neutral';
}

export function TradeTypePill({ type }: { type: string | null | undefined }) {
  if (!type) return <span className="muted">—</span>;
  return <Badge tone={tradeTypeTone(type)}>{type}</Badge>;
}

// Single source of truth for wallet-role → color. Each role gets a distinct hue (see
// --color-role-* in index.css); unknown roles fall back to muted. Typed by WalletRole
// so adding a role to the union without giving it a hue here fails to COMPILE.
const ROLE_COLOR: Record<WalletRole, string> = {
  dev: 'var(--color-role-dev)',
  bundler: 'var(--color-role-bundler)',
  treasury: 'var(--color-role-treasury)',
  trading: 'var(--color-role-trading)',
};

const KNOWN_ROLES = new Set<string>(Object.keys(ROLE_COLOR));

/** CSS var holding a role's accent color — for tinting summary cards, bars, etc. */
export function roleColorVar(role: string): string {
  return ROLE_COLOR[role.toLowerCase() as WalletRole] ?? 'var(--color-muted)';
}

export function RolePill({ role }: { role: string | null | undefined }) {
  if (!role) return <span className="muted">—</span>;
  const r = role.toLowerCase();
  const key = KNOWN_ROLES.has(r) ? r : 'unknown';
  return <span className={clsx('badge', `badge-role-${key}`)}>{role}</span>;
}
