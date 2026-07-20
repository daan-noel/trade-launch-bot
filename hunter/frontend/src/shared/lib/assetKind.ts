/**
 * Known-mint asset classification — mirrors `trading_core::models::asset`.
 *
 * Keep mint strings identical to Rust `config::constants::{USDC_MINT, WSOL_MINT}`.
 */

/** Circle USDC (Solana mainnet). */
export const USDC_MINT = 'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v';

/** Wrapped SOL mint. */
export const WSOL_MINT = 'So11111111111111111111111111111111111111112';

export type AssetKind = 'cash' | 'wrapped_sol' | 'meme';

/** Classify a mint. Unknown → meme. Prefer server `asset_kind` when present. */
export function assetKind(mint: string): AssetKind {
  if (mint === USDC_MINT) return 'cash';
  if (mint === WSOL_MINT) return 'wrapped_sol';
  return 'meme';
}

export function isCashMint(mint: string): boolean {
  return assetKind(mint) === 'cash';
}

/** Row is cash when the server tagged it, else fall back to the mint registry. */
export function isCashHolding(h: { mint_address: string; asset_kind?: AssetKind | null }): boolean {
  if (h.asset_kind === 'cash') return true;
  if (h.asset_kind === 'meme' || h.asset_kind === 'wrapped_sol') return false;
  return isCashMint(h.mint_address);
}
