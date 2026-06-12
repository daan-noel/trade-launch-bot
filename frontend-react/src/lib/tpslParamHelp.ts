/** Plain-language help copy for every TPSL rule parameter, shared by the TPSL1
 *  and TPSL2 add/edit modals (rendered as a ⓘ InfoTooltip next to each field)
 *  and by the rule-table column headers (rendered as a native `title` tooltip
 *  via {@link paramTip}). Keyed by the camelCase form-field name so the modals
 *  can look an entry up directly.
 *
 *  Wording goal: explain what each value actually DOES in simple terms anyone
 *  can follow — the bodies mirror the backend behaviour (token-fingerprint
 *  matching, scalp entry gates, and the exit ladder). TPSL1 uses the subset
 *  without the entry gates / cohort exit; TPSL2 uses all of them. */
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
      'How loosely the number filters above match — a plus/minus percent band around each value. Example: Initial Buy 1.0 with 10% tolerance matches tokens whose first buy is between 0.9 and 1.1 SOL. 0% means an exact match. Applies to Initial Buy, CU Limit, CU Price, Max SOL Cost and Spendable SOL In.',
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
  minAgeSecs: {
    title: 'Min Age (seconds)',
    body:
      "Wait until the token is at least this many seconds old (measured from its first trade) before buying, skipping the wild first moments after launch. Blank or 0 turns it off.",
  },
  minAliveSol: {
    title: 'Min Alive SOL',
    body:
      "Only buy if the token is still actively trading. 'Alive SOL' is the total SOL traded (buys + sells) in the last 10 seconds; require at least this much so you don't buy into a token that has already gone quiet. Blank or 0 turns it off.",
  },
  minOrganicSol: {
    title: 'Min Organic SOL',
    body:
      "Require real outside demand. 'Organic SOL' is the net SOL bought by wallets that are NOT part of the launch group (the cohort — the wallets that bought in the token's first moments). A high value means genuine new buyers, not just insiders. Blank or 0 turns it off.",
  },
  pullbackPct: {
    title: 'Pullback %',
    body:
      "First half of the 'higher-low' entry. Wait for the price to dip at least this percent below its recent high before considering a buy — this is the size of the pullback you want to see. Needs Higher-Low (s) set too. Blank or 0 turns it off.",
  },
  higherLowSecs: {
    title: 'Higher-Low (seconds)',
    body:
      "Second half of the 'higher-low' entry. After the first dip (see Pullback %), buy only once the price makes a second dip that bottoms HIGHER than the first — confirming buyers are stepping in. The two bottoms must be at least this many seconds apart, which filters out split-second fakes. Blank or 0 turns it off.",
  },
  maxCohortHeld: {
    title: 'Max Cohort Held',
    body:
      "Don't buy if the launch group is still holding too much. 'Cohort held' is how much of what the launch wallets originally bought they still hold (1.0 = full bag, 0.05 = they have sold 95%). Require it at or below this so the early insiders have mostly cashed out before you enter, lowering rug risk. Blank or 0 turns it off.",
  },
  minLiquiditySol: {
    title: 'Min Liquidity SOL',
    body:
      "Require at least this much REAL SOL in the pool before buying. Real liquidity is actual deposited SOL (not the bonding-curve 'virtual' number the creator can fake), so this avoids tokens you can't sell out of. Blank or 0 turns it off.",
  },
  minOrganicLiq: {
    title: 'Min Organic Liquidity SOL',
    body:
      "Like Min Liquidity, but counts only the liquidity that came from outside buyers — real pool SOL minus what the launch group put in. Makes sure the liquidity isn't just the dev's own seed money. Blank or 0 turns it off.",
  },

  // ── Exit gates: when to sell ──
  takeProfit: {
    title: 'Take Profit %',
    body:
      'Sell as soon as the price is this percent ABOVE your entry price. Example: 50 means take profit at +50%. This is a required exit.',
  },
  stopLoss: {
    title: 'Stop Loss %',
    body:
      'Sell as soon as the price is this percent BELOW your entry price, to cap your loss. Example: 20 means cut the loss at -20%. This is a required exit.',
  },
  trailingStopPct: {
    title: 'Trailing Stop %',
    body:
      'Locks in gains by following the peak. The bot remembers the highest price reached since you entered and sells if price falls this percent below that peak. Example: 30 with a peak of $10 sells at $7. Blank or 0 turns it off.',
  },
  timeStopSecs: {
    title: 'Time Stop (seconds)',
    body:
      'Sell after holding for this long, even if neither take-profit nor stop-loss has hit. Cuts loose positions that are just sitting flat. Blank or 0 turns it off.',
  },
  stallSecs: {
    title: 'Stall (seconds)',
    body:
      "Sell when momentum dies — when the price hasn't made a new high for this many seconds. Gets you out of a token that has stopped climbing. Blank or 0 turns it off.",
  },
  liquidityDropPct: {
    title: 'Liquidity Drop %',
    body:
      'Sell if the pool\'s REAL SOL liquidity crashes this percent below its highest point since you entered. Catches liquidity being pulled (a rug) that a price-based stop might miss. Example: 50 sells when real liquidity halves from its peak. Blank or 0 turns it off.',
  },
  cohortExitRatio: {
    title: 'Cohort Exit Ratio',
    body:
      'Rug early-warning (TPSL2). Sell when the launch group dumps down to this fraction of everything they ever bought. Example: 0.05 sells once the insiders have offloaded to 5% of their original holdings, front-running a coordinated dump. Blank or 0 turns it off.',
  },
} satisfies Record<string, ParamHelp>;

export type TpslParamKey = keyof typeof TPSL_PARAM_HELP;

/** A single plain-text string ("Title — body") for native `title=` header
 *  tooltips, which can't render the bold title separately like InfoTooltip. */
export function paramTip(key: TpslParamKey): string {
  const h = TPSL_PARAM_HELP[key];
  return `${h.title} — ${h.body}`;
}
