/**
 * Desktop (OS) notifications for Hunter Live — Chromium/Windows best path.
 *
 * Uses a notification-only service worker so we get:
 * - action buttons (Open Ops / Trade) — ignored on plain `new Notification()`
 * - status-colored icon + optional hero card image
 * - per-position `tag` + `renotify` so BuySubmitted→Holding→Exit updates one toast
 * - `requireInteraction` for failed / unconfirmed exits
 *
 * Falls back to page `Notification` when the SW is unavailable.
 */

export interface DesktopNotifyPayload {
  status: string;
  /** Short mint prefix already truncated for title, or full mint for actions. */
  mint: string;
  modeLabel: 'real' | 'paper';
  ruleName: string;
  /** Extra line (exit reason, disarm reason). */
  detail?: string | null;
  /** Ops deep-link (click / Open). */
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
  image?: string;
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

const STATUS_LABEL: Record<string, string> = {
  Armed: 'Armed',
  Disarmed: 'Disarmed',
  BuySubmitted: 'Buy submitted',
  Holding: 'Holding',
  ExitPending: 'Exit pending',
  End: 'Closed',
  ExitFailed: 'Exit failed',
  ExitUnconfirmed: 'Exit unconfirmed',
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

function roundRect(
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
}

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

  ctx.fillStyle = color;
  roundRect(ctx, 0, 0, 128, 128, 28);
  ctx.fill();
  ctx.fillStyle = '#0f0f0f';
  ctx.font = '700 64px system-ui, Segoe UI, sans-serif';
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.fillText(letter, 64, 68);

  const url = canvas.toDataURL('image/png');
  iconCache.set(status, url);
  return url;
}

/** Wide hero card — Chromium may show it; Windows Action Center often still prefers icon+text. */
function statusCardImage(payload: DesktopNotifyPayload): string | undefined {
  const canvas = document.createElement('canvas');
  canvas.width = 720;
  canvas.height = 200;
  const ctx = canvas.getContext('2d');
  if (!ctx) return undefined;

  const color = STATUS_COLOR[payload.status] ?? '#13ceaf';
  const label = STATUS_LABEL[payload.status] ?? payload.status;

  ctx.fillStyle = '#141414';
  ctx.fillRect(0, 0, 720, 200);

  ctx.fillStyle = color;
  roundRect(ctx, 24, 40, 120, 120, 24);
  ctx.fill();
  ctx.fillStyle = '#0f0f0f';
  ctx.font = '700 56px system-ui, Segoe UI, sans-serif';
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.fillText(label.charAt(0).toUpperCase(), 84, 104);

  ctx.textAlign = 'left';
  ctx.fillStyle = '#f5f5f5';
  ctx.font = '700 36px system-ui, Segoe UI, sans-serif';
  ctx.fillText(label, 168, 70);

  ctx.fillStyle = '#a3a3a3';
  ctx.font = '600 28px ui-monospace, Consolas, monospace';
  const mintShort =
    payload.mint.length > 12 ? `${payload.mint.slice(0, 8)}…` : payload.mint;
  ctx.fillText(`${payload.modeLabel}  ·  ${mintShort}`, 168, 112);

  const sub = payload.detail
    ? `${payload.ruleName}  ·  ${payload.detail}`
    : payload.ruleName;
  ctx.fillStyle = '#737373';
  ctx.font = '500 24px system-ui, Segoe UI, sans-serif';
  const clipped = sub.length > 48 ? `${sub.slice(0, 47)}…` : sub;
  ctx.fillText(clipped, 168, 152);

  return canvas.toDataURL('image/png');
}

function buildTitle(payload: DesktopNotifyPayload): string {
  const label = STATUS_LABEL[payload.status] ?? payload.status;
  const mintShort =
    payload.mint.length > 12 ? payload.mint.slice(0, 8) : payload.mint;
  return `${label} · ${mintShort}`;
}

function buildBody(payload: DesktopNotifyPayload): string {
  const parts = [`${payload.modeLabel}`, `"${payload.ruleName}"`];
  if (payload.detail) parts.push(payload.detail);
  return parts.join(' · ');
}

function buildActions(payload: DesktopNotifyPayload): NotifyAction[] {
  const actions: NotifyAction[] = [{ action: 'open', title: 'Open Ops' }];
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
  const image = statusCardImage(payload);
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
  if (image) options.image = image;

  const reg = await ensureNotificationSw();
  if (reg?.active || reg?.waiting || reg?.installing) {
    try {
      // lib.dom's NotificationOptions omits Chromium `actions` / `image`.
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
