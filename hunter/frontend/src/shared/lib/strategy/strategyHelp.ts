/** Plain-language ⓘ help for rule / fingerprint / sweep authoring.
 *
 *  Wording goal: detailed enough to use without reading the plan docs, still
 *  scannable. Bodies may use newlines (`whitespace-pre-line` in InfoTooltip).
 *  Mirrors engine semantics: entry AND / exit OR across metrics; within one
 *  metric `,` = AND, `|` = OR, `lo..hi` = inclusive range. */

export interface HelpTip {
  title: string;
  body: string;
}

// ── Condition grammar (one metric's text box) ────────────────────────────────

export const CONDITION_GRAMMAR_HELP: HelpTip = {
  title: 'How to write a condition',
  body: [
    'This box compares the live metric value to numbers you type. Leave it empty to ignore this metric.',
    '',
    'OPERATORS (one comparison):',
    '  >10   greater than 10',
    '  >=10  at least 10',
    '  <10   less than 10',
    '  <=10  at most 10',
    '  =10   equal (bucket — see metric’s =tol)',
    '  !=10  not equal (same bucket width)',
    '',
    'RANGE SHORTCUT — lo..hi',
    '  10..40  means  >= 10 AND <= 40  (inclusive).',
    '  Same as typing:  >=10, <=40',
    '  If you reverse it (40..10), bounds swap automatically → still 10..40.',
    '  On blur/save the field expands to >= / <= text; ".." is input sugar only.',
    '',
    'COMMA (,) = AND — all parts must hold (builds a band):',
    '  >10, <=30     → strictly above 10 and at most 30',
    '  >=10, <=40    → same meaning as 10..40',
    '',
    'PIPE (|) = OR — any arm may hold:',
    '  <30 | >=70    → below 30, OR at/above 70 (outside the middle)',
    '  10..40 | >=70 → inside [10,40], OR at/above 70',
    '',
    'SAME METRIC, CROSSED BOUNDS:',
    '  Typing <30, >=70 (AND) is impossible as one range — it auto-becomes',
    '  <30 | >=70 (OR). Feasible pairs like >5, <40 stay AND.',
    '',
    'Across different metrics, entry still ANDs and exit still ORs (see side tip).',
  ].join('\n'),
};

// ── Entry / exit sides ───────────────────────────────────────────────────────

export const SIDE_HELP = {
  entry: {
    title: 'Entry side — when to BUY',
    body: [
      'Runs after the fingerprint matches and the (token, rule) arm is live.',
      '',
      'ACROSS METRICS: AND — every metric you fill must be true at the same moment. Example: time > 5 AND liquidity > 10 → both required.',
      '',
      'WITHIN ONE METRIC: still use , (AND) / | (OR) / lo..hi as in the condition tip. Example: time 5..30 → enter only while age is between 5 and 30 seconds.',
      '',
      'Empty metric = ignore that metric. Empty whole entry side = buy as soon as the fingerprint arms (no extra wait).',
      '',
      'Monotonic tip: time only goes up. An entry like time < 30 can never succeed after 30s — the engine disarms that arm (derived unsatisfiable).',
    ].join('\n'),
  },
  exit: {
    title: 'Exit side — when to SELL',
    body: [
      'Checked while you hold a position, alongside take-profit and stop-loss.',
      '',
      'ACROSS METRICS: OR — any one filled metric can fire the sell (same idea as TP or SL firing alone). Example: stall > 15 OR trail > 20 → either is enough.',
      '',
      'WITHIN ONE METRIC: , = AND, | = OR, lo..hi = inclusive range. Examples:',
      '  liquidity 20..50     → sell while liquidity is inside that band',
      '  liquidity <30 | >=70 → sell if pool is thin OR very deep',
      '',
      'Empty whole exit side = only TP / SL / death close the trade (no metric exit).',
      '',
      'Close reasons OR together: TP hit OR SL hit OR any exit metric OR dead/migrated.',
    ].join('\n'),
  },
} as const satisfies Record<'entry' | 'exit', HelpTip>;

// ── Metric groups ────────────────────────────────────────────────────────────

export const GROUP_HELP: Record<string, HelpTip> = {
  m_snapshot: {
    title: 'm_snapshot — point-in-time facts',
    body: [
      'Always-on metrics that do not need a trailing window.',
      '',
      '• time — seconds since token creation (age).',
      '• liquidity — current pool SOL reserves.',
      '',
      'Use for age gates and pool-depth filters on entry or exit. Kind: static.',
    ].join('\n'),
  },
  m_price_path: {
    title: 'm_price_path — price behaviour',
    body: [
      'How price has been moving since the token (and your hold) has been alive.',
      '',
      '• stall — seconds since the price last set a new all-time high (how long since progress).',
      '• trail — % drop from the peak price seen so far (give-back from the high).',
      '',
      'No window_size_sec. Kind: static.',
    ].join('\n'),
  },
  m_time_window: {
    title: 'm_time_window — trailing flow',
    body: [
      'Sums buys/sells over the last N seconds (N = window_size_sec, required).',
      '',
      'Metrics (all SOL):',
      '• gross_flow — buys + sells (activity)',
      '• net_flow — buys − sells (direction)',
      '• buy — buys only',
      '• sell — sells only',
      '',
      'If you use any metric here, you must set window_size_sec (e.g. 10). Kind: dynamic.',
    ].join('\n'),
  },
};

// ── Per-metric meaning ───────────────────────────────────────────────────────

export const METRIC_HELP: Record<string, HelpTip> = {
  time: {
    title: 'time — age (seconds)',
    body: [
      'Seconds since the token was created. Starts near 0 and only increases (monotonic).',
      '',
      'Examples:',
      '  >5          wait until older than 5s',
      '  5..30       only while age is between 5 and 30s (same as >=5, <=30)',
      '  <30         must enter before 30s — after that this entry can never fire again (arm disarms)',
      '',
      'Typical entry gate to skip the first chaotic moments or to force an early sniper window.',
    ].join('\n'),
  },
  liquidity: {
    title: 'liquidity — pool SOL',
    body: [
      'Current bonding-curve / AMM SOL reserves (how deep the pool is right now).',
      '',
      'Examples:',
      '  >10              need at least ~10◎ in the pool',
      '  20..50           inside a band (expands to >=20, <=50)',
      '  <30 | >=70       outside band — thin OR very deep (comma form <30, >=70 auto-ORs)',
      '',
      'Common on exit for “rug thin” or “overheated depth” signals.',
    ].join('\n'),
  },
  stall: {
    title: 'stall — time since the all-time high (seconds)',
    body: [
      'Seconds since the price last set a NEW all-time high. Resets to ~0 only when a trade prints above the running peak; trades at or below it let the clock keep running.',
      '',
      'Pairs with trail off the same anchor: trail is HOW FAR below the high, stall is HOW LONG since it.',
      '',
      'Examples:',
      '  >15     no new high for 15s → the run has stalled out',
      '  <2      still making fresh highs',
      '',
      'Often used as an exit: sell once the token stops making progress.',
    ].join('\n'),
  },
  trail: {
    title: 'trail — drawdown from peak (%)',
    body: [
      'Percent drop from the highest price seen so far (high-water mark). 0 at the peak; grows as price gives back.',
      '',
      'Examples:',
      '  >20     sell after a 20% give-back from the peak',
      '  <5      still near the high',
      '',
      'Classic trailing-style exit without a separate trail parameter.',
    ].join('\n'),
  },
  gross_flow: {
    title: 'gross_flow — total volume (SOL)',
    body: [
      'Buys + sells in SOL over window_size_sec. Measures how much traded, not who won.',
      '',
      'Example: >5 with window 10 → more than 5◎ changed hands in the last 10 seconds.',
      '',
      'Needs window_size_sec on this group.',
    ].join('\n'),
  },
  net_flow: {
    title: 'net_flow — buy pressure (SOL)',
    body: [
      'Buys minus sells in SOL over the window. Positive = net buying; negative = net selling.',
      '',
      'Examples:',
      '  >2      strong net buying in the window',
      '  <0      net selling',
      '',
      'Needs window_size_sec on this group.',
    ].join('\n'),
  },
  buy: {
    title: 'buy — buy volume (SOL)',
    body: [
      'SOL spent on buys only inside the trailing window (sells ignored).',
      '',
      'Example: >3 → at least 3◎ of buys in the last window_size_sec seconds.',
      '',
      'Needs window_size_sec on this group.',
    ].join('\n'),
  },
  sell: {
    title: 'sell — sell volume (SOL)',
    body: [
      'SOL from sells only inside the trailing window (buys ignored).',
      '',
      'Example: >2 → heavy selling in the window — often an exit signal.',
      '',
      'Needs window_size_sec on this group.',
    ].join('\n'),
  },
};

/** Append unit / =tol / monotonic facts from the live registry spec. */
export function metricHelpBody(
  metric: string,
  spec?: { unit: string; eq_tolerance: number; monotonic: boolean },
): string {
  const base =
    METRIC_HELP[metric]?.body ??
    'Registry metric — type a condition in the box, or leave empty to ignore.';
  if (!spec) return base;
  const unit =
    spec.unit === 'seconds' ? 'seconds' : spec.unit === 'percent' ? 'percent' : 'SOL';
  const half = spec.eq_tolerance / 2;
  const bits = [
    '',
    `Unit: ${unit}.`,
    `= / != bucket: values within ±${half} ${unit} of the target count as equal (tol ${spec.eq_tolerance}).`,
  ];
  if (spec.monotonic) {
    bits.push(
      'Monotonic: value never decreases. An entry upper bound (e.g. time < 30) that is crossed permanently disarms the arm.',
    );
  }
  return `${base}${bits.join('\n')}`;
}

// ── Strict params ────────────────────────────────────────────────────────────

export const STRICT_PARAM_HELP: Record<string, HelpTip> = {
  window_size_sec: {
    title: 'window_size_sec — flow lookback',
    body: [
      'How many seconds of recent trades to include for m_time_window metrics (gross_flow, net_flow, buy, sell).',
      '',
      'Required if any metric in this group has a condition. Example: 10 → sum the last 10 seconds of flow.',
      '',
      'All m_time_window axes on the same side must share one window (sweep rejects conflicting windows).',
    ].join('\n'),
  },
};

// ── Rule editor fields ───────────────────────────────────────────────────────

export const RULE_FIELD_HELP = {
  name: {
    title: 'Rule name',
    body: [
      'Display label in tables, logs, and promote drafts. Does not affect matching or PnL.',
      '',
      'Pick something you’ll recognize later (e.g. “liq-exit outside 30/70”).',
    ].join('\n'),
  },
  mode: {
    title: 'Trade mode',
    body: [
      'paper — simulate fills from the live trade feed. No wallet, no on-chain spend. Use for testing rules.',
      '',
      'real — send actual buys/sells on-chain. Spends SOL from the configured wallet.',
      '',
      'You can change sizing while live; fingerprint + entry/exit conditions lock once the rule is active.',
    ].join('\n'),
  },
  buy: {
    title: 'Buy amount (SOL)',
    body: [
      'How much SOL this rule spends on each entry fill (one stake per position).',
      '',
      'Paper: simulated. Real: live on-chain spend. Must be > 0.',
    ].join('\n'),
  },
  maxConcurrent: {
    title: 'Max concurrent positions',
    body: [
      'Maximum open positions this rule may hold at the same time.',
      '',
      'While at the cap, new entries are blocked until something exits. Must be ≥ 1.',
    ].join('\n'),
  },
  maxTotal: {
    title: 'Max total entries',
    body: [
      'Lifetime number of entries allowed for the whole run of this rule.',
      '',
      '0 = unlimited. After N successful entries, the rule stops taking new tokens.',
    ].join('\n'),
  },
  fingerprint: {
    title: 'Fingerprint',
    body: [
      'Creation-time matcher: which brand-new tokens this rule is allowed to arm on (CU, first buy, labels, etc.).',
      '',
      'Flow: fingerprint matches → arm → entry conditions (if any) → buy → exit (TP/SL/metrics).',
      '',
      'Locked while the rule is live so live behaviour cannot silently change mid-run.',
    ].join('\n'),
  },
  takeProfit: {
    title: 'Take profit (%)',
    body: [
      'Sell when mark price is this percent above entry price.',
      '',
      'Examples: 100 → +100% (2× entry). 50 → +50%.',
      '',
      'ORs with stop-loss and exit metrics — any one can close. Leave empty to disable TP.',
    ].join('\n'),
  },
  stopLoss: {
    title: 'Stop loss (%)',
    body: [
      'Sell when mark price is this percent below entry price.',
      '',
      'Examples: 30 → −30% from entry. 50 → −50%.',
      '',
      'ORs with take-profit and exit metrics. Leave empty to disable SL.',
    ].join('\n'),
  },
  paramsJson: {
    title: 'Params JSON',
    body: [
      'Raw strategy params: take_profit, stop_loss, entry{…}, exit{…}.',
      '',
      'Each metric is a list of {operator, value} (one AND arm) or a nested list of arms for OR.',
      'Example liquidity outside band: [[{"operator":"<","value":30}],[{"operator":">=","value":70}]].',
      '',
      'Validated against the metric registry on save. Prefer the Builder tab unless you know the shape.',
    ].join('\n'),
  },
} as const satisfies Record<string, HelpTip>;

// ── Fingerprint fields ───────────────────────────────────────────────────────

export const FINGERPRINT_FIELD_HELP = {
  name: {
    title: 'Fingerprint name',
    body: [
      'Human label for this matcher. Rules pick a fingerprint by id; the name is for you.',
      '',
      'Many rules can share one fingerprint.',
    ].join('\n'),
  },
  cu_limit: {
    title: 'CU limit (exact)',
    body: [
      'Compute-unit limit on the token’s creation transaction — how much compute the launch tx requested.',
      '',
      'Matched exactly (not bucketed). Useful fingerprint of a particular launch bot/tool.',
      'Leave blank to ignore.',
    ].join('\n'),
  },
  cu_price: {
    title: 'CU price (exact)',
    body: [
      'Compute-unit price on the creation tx (priority fee per CU, micro-lamports).',
      '',
      'Matched exactly. Higher often means a more aggressive launch sniper.',
      'Leave blank to ignore.',
    ].join('\n'),
  },
  init_buy: {
    title: 'Initial buy (SOL, bucketed)',
    body: [
      'SOL spent on the creator’s very first buy at launch.',
      '',
      'Matched by bucket width: token matches if its first buy falls in the same [lo, hi) bin as this value.',
      'Example: value 1.0 with bucket 0.1 → matches first buys in [1.0, 1.1).',
      'Leave blank to ignore.',
    ].join('\n'),
  },
  max_cost: {
    title: 'Max SOL cost (bucketed)',
    body: [
      'Max SOL the creator set on the launch buy instruction (slippage / max cost field).',
      '',
      'Read from the creation tx; matched by the same bucket width as other SOL axes.',
      'Leave blank to ignore.',
    ].join('\n'),
  },
  spendable_in: {
    title: 'Spendable SOL in (bucketed)',
    body: [
      'Spendable SOL recorded on the launch instruction — rough fingerprint of creator wallet funds at launch.',
      '',
      'Bucket-matched like initial buy. Leave blank to ignore.',
    ].join('\n'),
  },
  first_slot_buy: {
    title: 'First-slot buy (SOL, bucketed)',
    body: [
      'Total buy SOL in the creation slot (all buys that slot, not only the creator).',
      '',
      'Settles only after the creation slot closes — matching may wait (deferred first-slot axes).',
      'Bucket-matched. Leave blank to ignore.',
    ].join('\n'),
  },
  first_slot_sell: {
    title: 'First-slot sell (SOL, bucketed)',
    body: [
      'Total sell SOL in the creation slot.',
      '',
      'Same deferred settlement as first-slot buy. Bucket-matched. Leave blank to ignore.',
    ].join('\n'),
  },
  bucket: {
    title: 'Bucket width (SOL)',
    body: [
      'Width of each [lo, hi) bin for continuous SOL fingerprint axes (init buy, max cost, spendable, first-slot buy/sell).',
      '',
      'Must match the width used in grouped sweep / creation-stats if you want “what I grouped = what I run”.',
      'Default 0.1◎. Exact axes (cu_limit, cu_price, ix_labels) ignore this.',
    ].join('\n'),
  },
  ix_labels: {
    title: 'Instruction labels (exact sequence)',
    body: [
      'JSON array of instruction labels on the creation transaction, in order.',
      '',
      'Example: ["Pump.Fun: Create","Pump.Fun: Buy"]',
      '',
      'Matched as an exact ordered sequence. Leave empty to skip this filter.',
    ].join('\n'),
  },
} as const satisfies Record<string, HelpTip>;

// ── Sweep config / axes ──────────────────────────────────────────────────────

export const SWEEP_FIELD_HELP = {
  method: {
    title: 'Sweep method',
    body: [
      'How combos are chosen inside each fingerprint group:',
      '',
      '• grid — every combination of axis values (full Cartesian product).',
      '• random — draw N random combos (good for huge grids).',
      '• refine — random coarse pass, then full grid on the top-K survivors per group.',
    ].join('\n'),
  },
  samples: {
    title: 'Samples (N)',
    body: [
      'Number of random combos to draw in random mode, or in the coarse stage of refine.',
      '',
      'Larger N = better coverage, more CPU/RAM.',
    ].join('\n'),
  },
  topK: {
    title: 'Top-K per group (refine)',
    body: [
      'After the coarse random pass, how many best combos in each group are kept for a full refine grid.',
      '',
      'Only used when Method = refine.',
    ].join('\n'),
  },
  minTokens: {
    title: 'Min tokens per group',
    body: [
      'Skip fingerprint groups that have fewer tokens than this.',
      '',
      'Filters out tiny noisy buckets so rankings are not dominated by 1–2 token flukes.',
    ].join('\n'),
  },
  tokenCap: {
    title: 'Token cap per group',
    body: [
      'Max tokens processed from one group (RAM/time guard).',
      '',
      'If a bucket has more tokens, extras are not scored.',
    ].join('\n'),
  },
  maxCombos: {
    title: 'Max combos per group',
    body: [
      'Hard ceiling on how many combos may be evaluated inside one group.',
      '',
      'Oversized axis products are rejected before the run starts (protects the box).',
    ].join('\n'),
  },
  ramReserve: {
    title: 'RAM reserve',
    body: [
      'Host RAM left free for the OS + desktop while the sweep runs.',
      '',
      'Every sizing ceiling is "host free RAM − this reserve", so a smaller reserve lets',
      'a run go wider on a box you are not using, and a bigger one keeps the machine more',
      'responsive.',
      '',
      'This is a preference, not a limit: if a run does not fit, the sweep degrades itself',
      'to fit (fewer threads, smaller batches) and tells you it did — it does not refuse.',
      'A tight reserve costs wall-clock, not the run.',
    ].join('\n'),
  },
  avx512: {
    title: 'AVX-512 exit scan',
    body: [
      'Runs the per-combo exit scan (stop-loss / take-profit / dead search) on the CPU’s',
      'AVX-512 vector unit, 8 prices per instruction, instead of one at a time.',
      '',
      'Results are byte-identical to the scalar path (a parity test proves it) — only the',
      'speed changes, so a run is comparable whether it was on or off.',
      '',
      'Lab-only, and honored only on a host that has AVX-512: elsewhere the run falls back',
      'to the scalar scan and says so. Combos with metric exit conditions use scalar either',
      'way (only the pure TP/SL search is vectorized).',
    ].join('\n'),
  },
  buyAmount: {
    title: 'Buy amount (sweep)',
    body: [
      'Assumed SOL stake per entry when scoring every combo.',
      '',
      'Same size for all combos so rankings compare strategy logic, not stake differences.',
    ].join('\n'),
  },
  curveOnly: {
    title: 'Curve only',
    body: [
      'When checked, only bonding-curve (pre-migration) trades feed the sweep.',
      '',
      'Post-migration AMM tape is ignored for scoring.',
    ].join('\n'),
  },
  axisValues: {
    title: 'Axis values (sweep grid)',
    body: [
      'This is NOT the same as a rule condition box.',
      '',
      'Here you list discrete picks for the grid, comma-separated:',
      '  5, 10, 20',
      '  off, 5, 10     — “off” = that combo omits this condition entirely',
      '',
      'SWEEP RANGE (generates a list of picks):',
      '  10..40 step 10  →  10, 20, 30, 40',
      'Each pick becomes its own combo value with this axis’s operator.',
      '',
      'Do not confuse with a RULE condition range:',
      '  In a metric condition box, 10..40 means one band (>=10 AND <=40).',
      '  In this axis values box, 10..40 step 10 means four separate thresholds to try.',
      '',
      'Same metric, two axes: feasible opposing ops → AND range combo; crossed ops → OR outside band.',
    ].join('\n'),
  },
  axisWindow: {
    title: 'Window (seconds)',
    body: [
      'Trailing window for m_time_window metrics on this axis (same meaning as window_size_sec on a rule).',
      '',
      'Every m_time_window axis on the same side must use the same window — conflicts are rejected.',
    ].join('\n'),
  },
  axisOp: {
    title: 'Operator',
    body: [
      'Comparison applied to each value on this axis when building a combo’s RuleParams.',
      '',
      'Supported: >  >=  <  <=  =  !=',
      '',
      'Example: op > and values 5, 10 → combos with time > 5 and time > 10 (separate combos).',
    ].join('\n'),
  },
} as const satisfies Record<string, HelpTip>;
