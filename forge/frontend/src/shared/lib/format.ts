import bs58 from 'bs58';

const LAMPORTS_PER_SOL = 1_000_000_000;

/** Lamports (exact native SOL) → human "0.05 SOL".
 *  `digits` is a MAX (not a fixed count) so trailing zeros are dropped:
 *  `0.0500 → "0.05 SOL"`, `0 → "0 SOL"`. */
export function formatSol(lamports: number | null | undefined, digits = 4): string {
  if (lamports == null) return '—';
  return `${lamportsToSol(lamports, digits)} SOL`;
}

/** Bare lamports → SOL number (no unit) for compact table cells.
 *  `digits` is a MAX so trailing zeros are dropped (`0.0500 → "0.05"`, `0 → "0"`). */
export function lamportsToSol(lamports: number | null | undefined, digits = 4): string {
  if (lamports == null) return '—';
  return (lamports / LAMPORTS_PER_SOL).toLocaleString('en-US', {
    maximumFractionDigits: digits,
    useGrouping: false,
  });
}

export function formatUsd(usd: number | null | undefined): string {
  if (usd == null) return '—';
  if (usd >= 1000) return `$${Math.round(usd).toLocaleString()}`;
  if (usd >= 1) return `$${usd.toFixed(2)}`;
  return `$${usd.toPrecision(3)}`;
}

/** Quote base units → human quote amount, given the quote's decimals.
 *  `digits` is a MAX (not a fixed count) so trailing zeros are dropped:
 *  `0.0200 → "0.02"`, `1.5 → "1.5"`, `0 → "0"`. */
export function quoteToHuman(
  base: number | null | undefined,
  decimals: number,
  digits = 4,
): string {
  if (base == null) return '—';
  return (base / 10 ** decimals).toLocaleString('en-US', {
    maximumFractionDigits: digits,
    useGrouping: false,
  });
}

export function formatCount(n: number | null | undefined): string {
  if (n == null) return '—';
  return n.toLocaleString();
}

/** `value.toFixed(decimals)` with trailing zeros (and a bare `.`) trimmed. */
export function formatDecimalTrim(value: number, decimals: number): string {
  const s = value.toFixed(decimals);
  if (!s.includes('.')) return s;
  const trimmed = s.replace(/0+$/, '').replace(/\.$/, '');
  return trimmed || s;
}

/**
 * Human-readable price for meme-token spot values (raw quote-per-base ratios).
 * `>= 1` trims to 6 decimals; tiny values render in engineering e-notation
 * (`1.23e-9`) so they never collapse to `0`. Ported from hunter's `utils/format`.
 */
export function formatPrice(value: number): string {
  if (value === 0) return '0';
  const abs = Math.abs(value);
  if (abs >= 1) return formatDecimalTrim(value, 6);

  const exponent = -Math.floor(Math.log10(abs));
  let engineeringExponent: number;
  if (exponent <= 3) engineeringExponent = -3;
  else if (exponent <= 6) engineeringExponent = -6;
  else if (exponent <= 9) engineeringExponent = -9;
  else if (exponent <= 12) engineeringExponent = -12;
  else if (exponent <= 15) engineeringExponent = -15;
  else return value.toExponential(6);

  const mantissa = value / 10 ** engineeringExponent;
  return `${formatDecimalTrim(mantissa, 6)}e${engineeringExponent}`;
}

/**
 * Full age breakdown down to the second (`30s`, `12m 30s`, `3h 12m 30s`,
 * `1D 3h 12m 30s`) — no unit collapsed. Day unit is uppercase `D`.
 */
export function formatAgePrecise(seconds: number): string {
  if (seconds < 0) return '?';
  const s = Math.floor(seconds);
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const parts: string[] = [];
  if (d > 0) parts.push(`${d}D`);
  if (d > 0 || h > 0) parts.push(`${h}h`);
  if (d > 0 || h > 0 || m > 0) parts.push(`${m}m`);
  parts.push(`${sec}s`);
  return parts.join(' ');
}

export function formatAge(iso: string | null | undefined, nowMs: number = Date.now()): string {
  if (!iso) return '—';
  const ms = nowMs - new Date(iso).getTime();
  const minutes = Math.floor(ms / 60_000);
  if (minutes < 1) return 'just now';
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

export function ageFromSecs(secs: number | null | undefined): string {
  if (secs == null) return '—';
  if (secs < 60) return `${secs}s`;
  const m = Math.floor(secs / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h`;
  return `${Math.floor(h / 24)}d`;
}

export function shortAddr(addr: string | null | undefined, lead = 4, tail = 4): string {
  if (!addr) return '—';
  if (addr.length <= lead + tail + 1) return addr;
  return `${addr.slice(0, lead)}…${addr.slice(-tail)}`;
}

export function solscanTx(sig: string): string {
  return `https://solscan.io/tx/${sig}`;
}

export function solscanMint(mint: string): string {
  return `https://solscan.io/token/${mint}`;
}

export function solscanAddr(addr: string): string {
  return `https://solscan.io/account/${addr}`;
}

export function gmgnMint(mint: string): string {
  return `https://gmgn.ai/sol/token/${mint}`;
}

/** Rewrites an `ipfs://…` URI to an HTTP gateway URL so browsers can fetch it;
 * passes through anything already HTTP(S). */
export function ipfsToHttp(uri: string | null | undefined): string | null {
  if (!uri) return null;
  return uri.startsWith('ipfs://') ? uri.replace('ipfs://', 'https://ipfs.io/ipfs/') : uri;
}

/** `trades_priced.tx_signature` arrives as a byte array — base58-encode it. */
export function formatSig(tx: number[] | string | null | undefined): string {
  if (typeof tx === 'string') return tx;
  if (!tx?.length) return '—';
  return bs58.encode(Uint8Array.from(tx));
}

/** Reads a File as base64 (strips the `data:<mime>;base64,` prefix). */
export function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = reader.result as string;
      resolve(result.slice(result.indexOf(',') + 1));
    };
    reader.onerror = () => reject(reader.error ?? new Error('file read failed'));
    reader.readAsDataURL(file);
  });
}
