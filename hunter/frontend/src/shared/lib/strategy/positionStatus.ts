import type { BadgeVariant } from 'components/ui/Badge';

/** Display labels for engine position statuses — table, modal, and chart card. */
export const OPEN_STATUS_LABEL: Record<string, string> = {
  BuySubmitted: 'Buy submitted',
  Holding: 'Holding',
  ExitPending: 'Exit pending',
  ExitUnconfirmed: 'Exit unconfirmed',
  ExitStuck: 'Exit stuck',
  End: 'End',
  EntryFailed: 'Entry failed',
};

/** Badge color for a raw engine status key (and Waiting). */
export function openStatusBadgeVariant(statusKey: string): BadgeVariant {
  switch (statusKey) {
    case 'ExitPending':
    case 'ExitUnconfirmed':
    case 'Waiting':
      return 'warning';
    case 'Holding':
    case 'End':
      return 'success';
    case 'BuySubmitted':
      return 'info';
    case 'ExitStuck':
    case 'EntryFailed':
      return 'danger';
    default:
      return 'neutral';
  }
}
