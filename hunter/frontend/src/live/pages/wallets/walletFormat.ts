import type { BadgeVariant } from 'components/ui/Badge';
import type { TraderWalletRow, WalletReadiness } from '@live/store/liveEndpoints';

export function shortKey(pubkey: string): string {
  if (pubkey.length <= 12) return pubkey;
  return `${pubkey.slice(0, 4)}...${pubkey.slice(-4)}`;
}

export function solText(sol: number | null | undefined, digits = 4): string {
  if (sol == null) return '-';
  return sol.toFixed(digits);
}

export function lamportsToSol(lamports: number): number {
  return lamports / 1e9;
}

export const READINESS_LABEL: Record<WalletReadiness, string> = {
  ready: 'Ready',
  needs_sol: 'Low SOL',
  needs_nonces: 'No nonces',
  init_failed: 'Start failed',
  no_key: 'Key not here',
};

export const READINESS_VARIANT: Record<WalletReadiness, BadgeVariant> = {
  ready: 'success',
  needs_sol: 'warning',
  needs_nonces: 'danger',
  init_failed: 'danger',
  no_key: 'neutral',
};

/** In use = primary or named by a rule; idle = active and neither. */
export type WalletTab = 'in_use' | 'idle' | 'archived';

export function walletTab(w: TraderWalletRow): WalletTab {
  if (w.status === 'archived') return 'archived';
  if (w.is_primary || w.rule_count > 0 || w.status === 'draining') return 'in_use';
  return 'idle';
}

/** One line for an activity row. */
export function activityText(kind: string, detail: Record<string, unknown>): string {
  const sol = (v: unknown) => (typeof v === 'number' ? `${lamportsToSol(v).toFixed(4)} SOL` : '');
  const key = (v: unknown) => (typeof v === 'string' ? shortKey(v) : '');
  switch (kind) {
    case 'created':
      return 'Created';
    case 'imported':
      return 'Imported';
    case 'registered_env':
      return 'Brought in from .env';
    case 'set_primary':
      return 'Set as primary';
    case 'renamed':
      return `Renamed ${String(detail.from ?? '')} to ${String(detail.to ?? '')}`;
    case 'nonces_created':
      return `Created ${String(detail.count ?? '')} nonce account(s)`;
    case 'nonces_attached':
      return `Attached ${String(detail.attached ?? '')} nonce account(s)`;
    case 'nonces_synced':
      return `Read from chain: attached ${String(detail.attached ?? 0)}, retired ${String(detail.retired ?? 0)} nonce account(s)`;
    case 'sol_sent':
      return `Sent ${sol(detail.lamports)} to ${key(detail.to)}${detail.external ? ' (outside)' : ''}`;
    case 'sol_received':
      return `Received ${sol(detail.lamports)} from ${key(detail.from)}`;
    case 'move_started':
      return `Move started to ${key(detail.to)}`;
    case 'move_done':
      return `Moved everything to ${key(detail.to)}`;
    case 'move_received':
      return `Received a move from ${key(detail.from)}`;
    case 'archived':
      return 'Archived';
    case 'restored':
      return 'Restored';
    case 'paused':
      return 'Paused: no new entries';
    case 'resumed':
      return 'Resumed entries';
    case 'key_revealed':
      return 'Private key revealed';
    case 'key_reveal_refused':
      return 'Key reveal refused: wrong passphrase';
    case 'restarted':
      return detail.init_error ? `Restart failed: ${String(detail.init_error)}` : 'Restarted';
    default:
      return kind;
  }
}
