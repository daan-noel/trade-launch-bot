/** Plain-language help copy for every TPSL rule parameter, shared by the TPSL1
 *  and TPSL2 add/edit modals (rendered as a ⓘ InfoTooltip next to each field)
 *  and by the rule-table column headers (rendered as a native `title` tooltip
 *  via {@link paramTip}). Keyed by the camelCase form-field name so the modals
 *  can look an entry up directly.
 *
 *  Wording goal: explain what each value actually DOES in simple terms anyone
 *  can follow — the bodies mirror the backend behaviour (token-fingerprint
 *  matching, scalp entry gates, and the exit ladder). TPSL1 uses the subset
 *  without the entry gates; TPSL2 uses all of them. */
export interface ParamHelp {
  title: string;
  body: string;
}

export const TPSL_PARAM_HELP = {
  // ── Token fingerprint: which brand-new token this rule matches at launch ──
  initialBuy: {
    title: 'Initial Buy (SOL)',
    body:
      "Picks tokens by how much SOL was spent on the token's very first buy (the creator's launch buy). Only tokens whose first buy is close to this number will match — how close is set by Tolerance. Leave blank to ignore this filter.",
  },
  tolerance: {
    title: 'Tolerance %',
    body:
      'Match band (±%) around each numeric token-fingerprint filter. Whole-percent, 0–100. Example: Initial Buy 1.0 with 10 matches first buys between 0.9 and 1.1 SOL; 0 means an exact match. Applies to Initial Buy, CU Limit, CU Price, Max SOL Cost and Spendable SOL In.',
  },
  cuLimit: {
    title: 'CU Limit',
    body:
      'Matches tokens by the compute-unit limit set in their creation transaction (how much processing the launch tx asked for). Acts as a fingerprint to recognise tokens launched by a particular bot or tool. Matched within Tolerance. Leave blank to ignore.',
  },
  cuPrice: {
    title: 'CU Price',
    body:
      'Matches tokens by the compute-unit price (the priority fee per unit, in micro-lamports) paid on the creation transaction. Higher means the creator paid more to land first — a fingerprint of how aggressively the token was sniped at launch. Matched within Tolerance. Leave blank to ignore.',
  },
  maxSolCost: {
    title: 'Max SOL Cost',
    body:
      "Matches tokens by the 'max SOL cost' the creator set in the launch instruction — the most SOL they were willing to spend on the first buy. Read straight from the creation transaction and matched within Tolerance. Leave blank to ignore.",
  },
  spendableSolIn: {
    title: 'Spendable SOL In',
    body:
      "Matches tokens by the 'spendable SOL' recorded in the launch instruction — roughly the SOL the creator's wallet had available at launch. A fingerprint of the creator's starting funds. Matched within Tolerance. Leave blank to ignore.",
  },
  ixLabels: {
    title: 'Instruction Labels',
    body:
      'A JSON list of instruction labels the token must carry, e.g. ["Pump.Fun: Buy"]. Right now this is a presence check: if you set any labels, the token must have at least one label to match. Leave blank to skip this filter; use the copy button to insert an example.',
  },

  // ── Sizing & limits ──
  buyAmount: {
    title: 'Buy Amount (SOL)',
    body:
      "How much SOL to spend each time this rule buys a token — your stake per position. In Paper mode it's simulated; in Real mode it's a live on-chain buy.",
  },
  maxConcurrentTokens: {
    title: 'Max Concurrent Tokens',
    body:
      "The most tokens this rule can hold at the same time. While you're holding this many, the rule stops buying new ones until some positions close. Leave blank for no limit.",
  },
  maxTotalTokens: {
    title: 'Max Total Tokens',
    body:
      'The total number of tokens this rule may trade over the whole run. Once it has entered this many, it stops taking any new tokens. Leave blank for no limit.',
  },

  // ── Entry gates · scalp continuation (TPSL2 only — decide WHEN to buy) ──
  // Each gate is a condition on the live trade stream; the bot buys on the first
  // trade where EVERY gate you set is satisfied. Blank or 0 turns a gate off.
  minAgeSecs: {
    title: 'Min Age (seconds)',
    body:
      "Don't buy until the token is at least this many seconds old (age = time since its first trade), skipping the chaotic launch spike. Example: 10 → ignore the first 10 seconds after launch. Blank or 0 turns it off.",
  },
  maxAgeSecs: {
    title: 'Max Age (seconds)',
    body:
      "Stop buying once the token is older than this many seconds (age from its first trade), so you only scalp the early window. With Min Age this forms the window [Min Age, Max Age] and must be larger than Min Age. Example: Min 10 + Max 60 → only buy while the token is between 10 and 60 seconds old. Blank or 0 means no ceiling — entries stay open until the token dies.",
  },
  minAliveSol: {
    title: 'Min Alive SOL',
    body:
      "Only buy while the token is still actively trading. Alive SOL = total SOL traded (buys + sells) in the last 10 seconds. Example: 5 → require ≥ 5 SOL of trading in the last 10s, so you skip tokens that have already gone quiet. Blank or 0 turns it off.",
  },
  minNetBuySol: {
    title: 'Min Net Buy SOL',
    body:
      "Require real net demand. Net Buy SOL = every buy minus every sell so far, any wallet — the SOL that has actually flowed IN. Example: 8 → enter only after a net +8 SOL has been bought, proving genuine buying pressure rather than a single launch trade. Sells subtract, so a token that pumped then dumped won't qualify. This measures demand (a flow), NOT how much SOL is in the pool — for that use Min Liquidity SOL. Blank or 0 turns it off.",
  },
  pullbackPct: {
    title: 'Pullback %',
    body:
      "First half of the 'higher-low' entry: the minimum dip off the recent high that counts as a real pullback. Example: 15 → the price must drop at least 15% from its high before the pattern can arm. Whole-percent, 0–100. Needs Higher-Low (s) set too. Blank or 0 turns it off.",
  },
  higherLowSecs: {
    title: 'Higher-Low (seconds)',
    body:
      "Second half of the 'higher-low' entry. After the first dip (see Pullback %), buy only once the price makes a SECOND dip that bottoms HIGHER than the first — confirming buyers are stepping in. The two bottoms must be at least this many seconds apart, which filters split-second fakes. Example: 10 → the two lows must be ≥ 10s apart. Blank or 0 turns it off.",
  },
  minLiquiditySol: {
    title: 'Min Liquidity SOL',
    body:
      "Require at least this much REAL SOL in the pool before buying. Real liquidity = actual deposited SOL, not the bonding-curve 'virtual' figure a creator can inflate. Example: 5 → skip any token with under 5 SOL of real liquidity that you couldn't sell back out of. This measures pool size (a level), NOT demand — for that use Min Net Buy SOL. Blank or 0 turns it off.",
  },

  // ── Exit gates: when to sell ──
  takeProfit: {
    title: 'Take Profit %',
    body:
      'Sell when price is this percent ABOVE your entry. Whole-percent and UNBOUNDED (a gain has no cap): 50 = exit at +50%, 250 = exit at +250% (3.5×). Must be greater than 0. Required exit.',
  },
  stopLoss: {
    title: 'Stop Loss %',
    body:
      'Sell when price is this percent BELOW your entry, to cap your loss. Whole-percent, 0–100: 20 = cut the loss at −20%. Required exit.',
  },
  trailingStopPct: {
    title: 'Trailing Stop %',
    body:
      'Locks in gains by following the peak: sell if price falls this percent below the highest price reached since entry. Whole-percent, 0–100: 30 with a peak of $10 sells at $7. Blank or 0 turns it off.',
  },
  timeStopSecs: {
    title: 'Time Stop (seconds)',
    body:
      'Sell after holding this long, even if neither take-profit nor stop-loss has hit — cuts loose a position just sitting flat. Example: 300 → force an exit 5 minutes after entry. Blank or 0 turns it off.',
  },
  stallSecs: {
    title: 'Stall (seconds)',
    body:
      "Sell when momentum dies — when the price hasn't set a new high for this many seconds — to get out of a token that stopped climbing. Example: 30 → exit after 30s with no new high. Blank or 0 turns it off.",
  },
  liquidityDropPct: {
    title: 'Liquidity Drop %',
    body:
      "Sell if the pool's REAL SOL reserves crash this percent below their peak since entry. Catches liquidity being pulled (a rug) a price stop might miss. Whole-percent, 0–100: 50 sells when real liquidity halves from its peak. Blank or 0 turns it off.",
  },
} satisfies Record<string, ParamHelp>;

export type TpslParamKey = keyof typeof TPSL_PARAM_HELP;

/** A single plain-text string ("Title — body") for native `title=` header
 *  tooltips, which can't render the bold title separately like InfoTooltip. */
export function paramTip(key: TpslParamKey): string {
  const h = TPSL_PARAM_HELP[key];
  return `${h.title} — ${h.body}`;
}
