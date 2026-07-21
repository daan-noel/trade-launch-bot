/**
 * Desktop (OS) notifications for Hunter Live — Chromium/Windows best path.
 *
 * Uses a notification-only service worker so we get:
 * - action buttons (Ops / Trade) — ignored on plain `new Notification()`
 * - status-colored icon (text stays the primary signal)
 * - per-position `tag` + `renotify` so BuySubmitted→Holding→Exit updates one toast
 * - `requireInteraction` for failed / unconfirmed exits
 *
 * Falls back to page `Notification` when the SW is unavailable.
 *
 * Copy targets (desktop glanceability): title ≤ ~40 chars, body ≤ ~90 chars.
 * No hero `image` — Windows crops it and it only duplicated title/body.
 */

import { exitReasonLabel } from 'lib/strategy/exitReason';

export interface DesktopNotifyPayload {
  status: string;
  /** Full mint; truncated for title/body. */
  mint: string;
  modeLabel: 'real' | 'paper';
  ruleName: string;
  /** Extra line (exit reason, disarm reason). */
  detail?: string | null;
  /** Ops deep-link (click / Ops). */
  href: string;
  /** Optional Trade desk link. */
  tradeHref?: string | null;
  /** Stable id so lifecycle updates replace the prior toast. */
  tag: string;
}

/** Chromium notification action — not always present in lib.dom typings. */
interface NotifyAction {
  action: string;
  title: string;
}

/** Options accepted by `ServiceWorkerRegistration.showNotification`. */
interface SwNotifyOptions extends NotificationOptions {
  renotify?: boolean;
  actions?: NotifyAction[];
  data?: {
    href: string;
    tradeHref: string | null;
    status: string;
  };
}

const STATUS_COLOR: Record<string, string> = {
  Armed: '#94a3b8',
  Disarmed: '#64748b',
  BuySubmitted: '#38bdf8',
  Holding: '#22c55e',
  ExitPending: '#f59e0b',
  End: '#13ceaf',
  ExitFailed: '#ef4444',
  ExitUnconfirmed: '#f97316',
};

/** Short OS-facing labels — event first, scannable in ~2 words. */
const STATUS_LABEL: Record<string, string> = {
  Armed: 'Armed',
  Disarmed: 'Disarmed',
  BuySubmitted: 'Buying',
  Holding: 'Holding',
  ExitPending: 'Exiting',
  End: 'Closed',
  ExitFailed: 'Exit failed',
  ExitUnconfirmed: 'Unconfirmed',
};

const iconCache = new Map<string, string>();
const CRITICAL = new Set(['ExitFailed', 'ExitUnconfirmed']);

let swReg: ServiceWorkerRegistration | null | undefined;

/** Register the notification SW once (live app only). */
export async function ensureNotificationSw(): Promise<ServiceWorkerRegistration | null> {
  if (swReg !== undefined) return swReg;
  if (!('serviceWorker' in navigator)) {
    swReg = null;
    return null;
  }
  try {
    const reg = await navigator.serviceWorker.register('/sw-notifications.js', {
      scope: '/',
    });
    await navigator.serviceWorker.ready;
    swReg = reg;
    return reg;
  } catch {
    swReg = null;
    return null;
  }
}

function mintShort(mint: string): string {
  return mint.length > 8 ? mint.slice(0, 8) : mint;
}

function clip(s: string, max: number): string {
  const t = s.trim();
  if (t.length <= max) return t;
  return `${t.slice(0, Math.max(1, max - 1))}…`;
}

function fillRoundRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
) {
  const rr = Math.min(r, w / 2, h / 2);
  ctx.beginPath();
  ctx.moveTo(x + rr, y);
  ctx.arcTo(x + w, y, x + w, y + h, rr);
  ctx.arcTo(x + w, y + h, x, y + h, rr);
  ctx.arcTo(x, y + h, x, y, rr);
  ctx.arcTo(x, y, x + w, y, rr);
  ctx.closePath();
  ctx.fill();
}

/** Compact detail for the body — exit reasons use Ops badge vocabulary. */
function formatDetail(status: string, detail: string | null | undefined): string | null {
  if (!detail) return null;
  if (status === 'End' || status === 'ExitFailed' || status === 'ExitUnconfirmed') {
    return exitReasonLabel(detail);
  }
  return clip(detail, 36);
}

/**
 * Minimal status glyph: soft dark tile + colored disk.
 * Color carries urgency; letter is a secondary cue (colorblind / collapsed trays).
 */
function statusIconDataUrl(status: string): string {
  const cached = iconCache.get(status);
  if (cached) return cached;

  const color = STATUS_COLOR[status] ?? '#13ceaf';
  const letter = (STATUS_LABEL[status] ?? status).charAt(0).toUpperCase() || '?';
  const canvas = document.createElement('canvas');
  canvas.width = 128;
  canvas.height = 128;
  const ctx = canvas.getContext('2d');
  if (!ctx) return '/favicon-live.svg';

  ctx.fillStyle = '#18181b';
  fillRoundRect(ctx, 0, 0, 128, 128, 28);

  ctx.beginPath();
  ctx.arc(64, 64, 40, 0, Math.PI * 2);
  ctx.fillStyle = color;
  ctx.fill();

  ctx.fillStyle = '#0a0a0a';
  ctx.font = '600 44px "Segoe UI", system-ui, sans-serif';
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.fillText(letter, 64, 66);

  const url = canvas.toDataURL('image/png');
  iconCache.set(status, url);
  return url;
}

/** Title: what happened + which mint. ~25–40 chars. */
function buildTitle(payload: DesktopNotifyPayload): string {
  const label = STATUS_LABEL[payload.status] ?? payload.status;
  return `${label} · ${mintShort(payload.mint)}`;
}

/**
 * Body: mode · rule · optional detail.
 * One glance line; no quotes, no filler.
 */
function buildBody(payload: DesktopNotifyPayload): string {
  const mode = payload.modeLabel === 'real' ? 'Real' : 'Paper';
  const rule = clip(payload.ruleName, 28);
  const detail = formatDetail(payload.status, payload.detail);
  return detail ? `${mode} · ${rule} · ${detail}` : `${mode} · ${rule}`;
}

function buildActions(payload: DesktopNotifyPayload): NotifyAction[] {
  const actions: NotifyAction[] = [{ action: 'open', title: 'Ops' }];
  if (payload.tradeHref) {
    actions.push({ action: 'trade', title: 'Trade' });
  }
  const max =
    typeof Notification !== 'undefined' && 'maxActions' in Notification
      ? (Notification as typeof Notification & { maxActions: number }).maxActions
      : 2;
  return actions.slice(0, Math.max(0, max));
}

/**
 * Show one OS notification. Prefer SW (actions); fall back to page Notification.
 * No-op when permission is not granted.
 */
export async function showDesktopNotify(payload: DesktopNotifyPayload): Promise<void> {
  if (typeof Notification === 'undefined' || Notification.permission !== 'granted') {
    return;
  }

  const title = buildTitle(payload);
  const body = buildBody(payload);
  const icon = statusIconDataUrl(payload.status);
  const critical = CRITICAL.has(payload.status);
  const actions = buildActions(payload);

  const options: SwNotifyOptions = {
    body,
    icon,
    badge: '/favicon-live.svg',
    tag: payload.tag,
    renotify: true,
    silent: !critical,
    requireInteraction: critical,
    data: {
      href: payload.href,
      tradeHref: payload.tradeHref ?? null,
      status: payload.status,
    },
  };

  const reg = await ensureNotificationSw();
  if (reg?.active || reg?.waiting || reg?.installing) {
    try {
      // lib.dom's NotificationOptions omits Chromium `actions`.
      await reg.showNotification(title, { ...options, actions } as NotificationOptions);
      return;
    } catch {
      /* fall through to page Notification */
    }
  }

  // Page constructor: no actions, but icon/tag/body still work.
  const n = new Notification(title, options);
  n.onclick = () => {
    window.focus();
    window.dispatchEvent(
      new CustomEvent('hunter-notification-navigate', { detail: { href: payload.href } }),
    );
    n.close();
  };
}
