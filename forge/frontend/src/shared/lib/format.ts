import bs58 from 'bs58';

const LAMPORTS_PER_SOL = 1_000_000_000;

/** Lamports (exact native SOL) → human "0.0500 SOL". */
export function formatSol(lamports: number | null | undefined, digits = 4): string {
  if (lamports == null) return '—';
  return `${(lamports / LAMPORTS_PER_SOL).toFixed(digits)} SOL`;
}

/** Bare lamports → SOL number (no unit) for compact table cells. */
export function lamportsToSol(lamports: number | null | undefined, digits = 4): string {
  if (lamports == null) return '—';
  return (lamports / LAMPORTS_PER_SOL).toFixed(digits);
}

export function formatUsd(usd: number | null | undefined): string {
  if (usd == null) return '—';
  if (usd >= 1000) return `$${Math.round(usd).toLocaleString()}`;
  if (usd >= 1) return `$${usd.toFixed(2)}`;
  return `$${usd.toPrecision(3)}`;
}

/** Quote base units → human quote amount, given the quote's decimals. */
export function quoteToHuman(
  base: number | null | undefined,
  decimals: number,
  digits = 4,
): string {
  if (base == null) return '—';
  return (base / 10 ** decimals).toFixed(digits);
}

export function formatCount(n: number | null | undefined): string {
  if (n == null) return '—';
  return n.toLocaleString();
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
