/**
 * Desktop (OS) notifications for Hunter Live — Chromium/Windows best path.
 *
 * The OS owns chrome/fonts — we only control icon, title, body, sound, actions.
 * Design targets: saturated status tile + geometric mark (readable at ~16–48px),
 * mint-first title, one quiet body line. No hero `image` (Windows crops poorly).
 *
 * Lifecycle: per-position `tag` quietly replaces; `renotify` + sound only for
 * money/critical statuses. Web Lock claim prevents multi-tab double-fire.
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

/** Saturated fills — must read as a status chip when Windows shrinks the icon. */
const STATUS_COLOR: Record<string, string> = {
  Armed: '#94a3b8',
  Disarmed: '#64748b',
  BuySubmitted: '#0ea5e9',
  Holding: '#22c55e',
  ExitPending: '#f59e0b',
  End: '#13ceaf',
  ExitFailed: '#ef4444',
  ExitUnconfirmed: '#f97316',
};

/** Short OS-facing labels — one glance verb. */
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
/** Failures always re-alert; Holding is the fill confirmation operators watch for. */
const CRITICAL = new Set(['ExitFailed', 'ExitUnconfirmed']);
const AUDIBLE = new Set(['Holding', 'ExitFailed', 'ExitUnconfirmed']);

/** Hold the per-event lock long enough for sibling tabs to see the same SSE frame. */
const CLAIM_HOLD_MS = 750;
const CLAIM_LS_PREFIX = 'mt:desktop-notify-claim:';

let swReg: ServiceWorkerRegistration | null | undefined;
/** In-flight SW register — collapses concurrent first-alert races. */
let swRegPending: Promise<ServiceWorkerRegistration | null> | null = null;

/** Register the notification SW once (live app only). */
export async function ensureNotificationSw(): Promise<ServiceWorkerRegistration | null> {
  if (swReg !== undefined) return swReg;
  if (swRegPending) return swRegPending;
  if (!('serviceWorker' in navigator)) {
    swReg = null;
    return null;
  }
  swRegPending = (async () => {
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
    } finally {
      swRegPending = null;
    }
  })();
  return swRegPending;
}

/**
 * Only one tab should call `showNotification` for a given (tag, status).
 * Each Live tab has its own SSE connection; without a claim, N tabs → N OS alerts
 * (especially painful when `renotify` is on for critical exits).
 */
async function withSoleTabClaim(key: string, fn: () => Promise<void>): Promise<void> {
  const locks = navigator.locks;
  if (locks?.request) {
    await locks.request(`hunter-desktop-notify:${key}`, { ifAvailable: true }, async (lock) => {
      if (!lock) return;
      await fn();
      await new Promise<void>((r) => setTimeout(r, CLAIM_HOLD_MS));
    });
    return;
  }

  // Fallback when Web Locks is missing — best-effort localStorage stamp.
  const lsKey = `${CLAIM_LS_PREFIX}${key}`;
  try {
    const now = Date.now();
    const prev = Number(localStorage.getItem(lsKey) ?? '');
    if (Number.isFinite(prev) && now - prev < CLAIM_HOLD_MS) return;
    localStorage.setItem(lsKey, String(now));
  } catch {
    /* private mode — proceed */
  }
  await fn();
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

/** Dark ink on saturated tile — matches live favicon contrast. */
const MARK = '#0a0a0a';

/** Geometric mark for a status — no letters (those blur at tray size). */
function drawStatusMark(ctx: CanvasRenderingContext2D, status: string): void {
  ctx.strokeStyle = MARK;
  ctx.fillStyle = MARK;
  ctx.lineCap = 'round';
  ctx.lineJoin = 'round';
  ctx.lineWidth = 11;

  switch (status) {
    case 'Armed': {
      // Target ring — echoes favicon-live.
      ctx.lineWidth = 9;
      ctx.beginPath();
      ctx.arc(64, 64, 28, 0, Math.PI * 2);
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(64, 64, 8, 0, Math.PI * 2);
      ctx.fill();
      break;
    }
    case 'Disarmed': {
      ctx.lineWidth = 9;
      ctx.beginPath();
      ctx.arc(64, 64, 28, 0.35 * Math.PI, 1.85 * Math.PI);
      ctx.stroke();
      ctx.beginPath();
      ctx.moveTo(48, 64);
      ctx.lineTo(80, 64);
      ctx.stroke();
      break;
    }
    case 'BuySubmitted': {
      // Chevron up.
      ctx.beginPath();
      ctx.moveTo(40, 78);
      ctx.lineTo(64, 44);
      ctx.lineTo(88, 78);
      ctx.stroke();
      break;
    }
    case 'Holding': {
      // Check.
      ctx.beginPath();
      ctx.moveTo(36, 66);
      ctx.lineTo(56, 86);
      ctx.lineTo(96, 42);
      ctx.stroke();
      break;
    }
    case 'ExitPending': {
      // Chevron down.
      ctx.beginPath();
      ctx.moveTo(40, 50);
      ctx.lineTo(64, 84);
      ctx.lineTo(88, 50);
      ctx.stroke();
      break;
    }
    case 'End': {
      // Done bar.
      ctx.lineWidth = 12;
      ctx.beginPath();
      ctx.moveTo(38, 64);
      ctx.lineTo(90, 64);
      ctx.stroke();
      break;
    }
    case 'ExitFailed': {
      // X.
      ctx.beginPath();
      ctx.moveTo(42, 42);
      ctx.lineTo(86, 86);
      ctx.moveTo(86, 42);
      ctx.lineTo(42, 86);
      ctx.stroke();
      break;
    }
    case 'ExitUnconfirmed': {
      // Bang.
      ctx.lineWidth = 12;
      ctx.beginPath();
      ctx.moveTo(64, 34);
      ctx.lineTo(64, 78);
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(64, 96, 7, 0, Math.PI * 2);
      ctx.fill();
      break;
    }
    default: {
      ctx.beginPath();
      ctx.arc(64, 64, 22, 0, Math.PI * 2);
      ctx.fill();
    }
  }
}

/**
 * Full-bleed status tile + geometric mark.
 * Color is the primary signal; mark stays legible when Windows shrinks the icon.
 */
function statusIconDataUrl(status: string): string {
  const cached = iconCache.get(status);
  if (cached) return cached;

  const color = STATUS_COLOR[status] ?? '#13ceaf';
  const canvas = document.createElement('canvas');
  canvas.width = 128;
  canvas.height = 128;
  const ctx = canvas.getContext('2d');
  if (!ctx) return '/favicon-live.svg';

  ctx.fillStyle = color;
  fillRoundRect(ctx, 0, 0, 128, 128, 28);
  drawStatusMark(ctx, status);

  const url = canvas.toDataURL('image/png');
  iconCache.set(status, url);
  return url;
}

/** Title: mint identity first, then status — ~20–36 chars. */
function buildTitle(payload: DesktopNotifyPayload): string {
  const label = STATUS_LABEL[payload.status] ?? payload.status;
  return `${mintShort(payload.mint)} · ${label}`;
}

/**
 * Body: mode · rule · optional detail.
 * One quiet line under the title — no quotes, no filler.
 */
function buildBody(payload: DesktopNotifyPayload): string {
  const mode = payload.modeLabel === 'real' ? 'REAL' : 'PAPER';
  const rule = clip(payload.ruleName, 32);
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
 * No-op when permission is not granted. Lifecycle updates share `tag` and replace
 * quietly; only critical statuses re-alert (`renotify`).
 */
export async function showDesktopNotify(payload: DesktopNotifyPayload): Promise<void> {
  if (typeof Notification === 'undefined' || Notification.permission !== 'granted') {
    return;
  }

  await withSoleTabClaim(`${payload.tag}:${payload.status}`, async () => {
    const title = buildTitle(payload);
    const body = buildBody(payload);
    const icon = statusIconDataUrl(payload.status);
    const critical = CRITICAL.has(payload.status);
    const audible = AUDIBLE.has(payload.status);
    const actions = buildActions(payload);

    const options: SwNotifyOptions = {
      body,
      icon,
      badge: '/favicon-live.svg',
      tag: payload.tag,
      // `tag` alone replaces the tray entry; `renotify` re-pops — keep for
      // failures only. Holding gets sound without re-spamming every replace.
      renotify: critical,
      silent: !audible,
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
  });
}
