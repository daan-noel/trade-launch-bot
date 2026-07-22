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
      'OVERLAP GATE: if any exit metric is already true at the same moment, the engine does not buy (would sell on the next tick). Keep entry and exit bands disjoint.',
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
      '',
      'If an exit metric is already true when entry would fire, entry is refused until that exit clears (see entry overlap gate).',
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
  m_price_lifetime: {
    title: 'm_price_lifetime — price behaviour',
    body: [
      'How price has been moving since the token (and your hold) has been alive.',
      '',
      '• stall — seconds since the price last set a new all-time high (how long since progress).',
      '• trail — % drop from the peak price seen so far (give-back from the high).',
      '',
      'No window_size_sec. Kind: static.',
    ].join('\n'),
  },
  m_price_window: {
    title: 'm_price_window — rolling price extrema',
    body: [
      'Price position relative to the highest/lowest print over the last N seconds',
      '(N = window_size_sec, required). Unlike m_price_lifetime (lifetime peak), this',
      'high/low rolls forward, so it reads short dips inside an otherwise-hot token.',
      '',
      '• trail — % below the rolling-window high (the dip-buy trigger).',
      '• rise — % above the rolling-window low (breakout/momentum).',
      '',
      'Empty window (no trade for N s) ⇒ NaN (never fires). Kind: dynamic.',
    ].join('\n'),
  },
  m_flow_window: {
    title: 'm_flow_window — trailing flow',
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
  m_flow_split: {
    title: 'm_flow_split — volume vs organic (lifetime)',
    body: [
      'Splits every trade into volume-side (creator tooling) vs organic, using the',
      'fingerprint\'s volume_ix_patterns + wallet contagion + creator wallet.',
      '',
      'Metrics (SOL unless noted): vol_buy/sell/net/gross, nonvol_*, vol_share (%).',
      'Unconfigured fingerprint ⇒ all NaN (conditions never fire). Kind: static.',
    ].join('\n'),
  },
  m_flow_split_window: {
    title: 'm_flow_split_window — volume vs organic (trailing)',
    body: [
      'Same split as m_flow_split, but over the last N seconds (window_size_sec).',
      'Reads the same volume_ix_patterns from the fingerprint (no duplicate config).',
      '',
      'Metric names mirror m_flow_split (vol_*, nonvol_*, vol_share). Kind: dynamic.',
    ].join('\n'),
  },
  m_position: {
    title: 'm_position — your open position (EXIT ONLY)',
    body: [
      'Metrics anchored on YOUR entry fill — they only exist while you hold, so this',
      'group is exit-only (hidden on the entry side).',
      '',
      '• retrace — % below the highest price since entry (the trailing stop).',
      '• pnl — signed % vs your entry price (take_profit / stop_loss desugar into this).',
      '• held — seconds since entry (a time-stop).',
      '',
      'Before entry these read NaN. Kind: static.',
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
    title: 'trail — drawdown from the high (%)',
    body: [
      'Percent below the high-water mark. 0 at the high; grows as price gives back.',
      '',
      'Two groups use this name:',
      '• m_price_lifetime.trail — vs the LIFETIME peak (classic trailing exit).',
      '• m_price_window.trail — vs the ROLLING window high (the dip-buy entry).',
      '',
      'Examples:',
      '  >=12    (window) 12%+ below the recent high → buy the dip',
      '  >20     (lifetime) sell after a 20% give-back from the peak',
    ].join('\n'),
  },
  rise: {
    title: 'rise — climb from the rolling low (%)',
    body: [
      'Percent above the lowest price in the last window_size_sec (m_price_window).',
      '0 at the low; grows as price recovers/breaks out.',
      '',
      'Example:  >=15   15%+ above the recent low → momentum/breakout entry.',
    ].join('\n'),
  },
  retrace: {
    title: 'retrace — give-back since entry (%)  [exit only]',
    body: [
      'Percent below the highest price seen SINCE YOUR ENTRY — a trailing stop off the',
      'post-entry peak. At entry the peak is your fill, so before any run-up it measures',
      'the drop from entry (a soft stop); after a run-up it trails the new peak.',
      '',
      'Example:  >=3    sell on a 3% pullback off the since-entry high.',
    ].join('\n'),
  },
  pnl: {
    title: 'pnl — profit/loss vs entry (%)  [exit only]',
    body: [
      'Signed percent vs your entry price. Positive = in profit, negative = underwater.',
      'take_profit / stop_loss are shorthand for this (pnl >= tp / pnl <= −sl).',
      '',
      'Examples:',
      '  >=100   +100% take-profit',
      '  <=-25   −25% catastrophe stop',
    ].join('\n'),
  },
  held: {
    title: 'held — time in position (seconds)  [exit only]',
    body: [
      'Seconds since your entry fill. Only increases while you hold.',
      '',
      'Example:  >=60   time-stop: bail if still holding after 60s.',
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

  // m_flow_split / m_flow_split_window — same JSON names; registry appends unit/tol/monotonic.
  vol_buy: {
    title: 'vol_buy — volume-side buys (SOL)',
    body: [
      'SOL spent on buys classified as volume-side (creator tooling / wash).',
      '',
      'A trade is volume-side if: its ix_labels match a volume_ix_patterns row,',
      'OR its wallet was already tagged volume on this token, OR it is the creator.',
      '',
      'Examples:',
      '  >2     heavy volume-side buying (lifetime or window)',
      '  <0.5   little tooling buy pressure',
      '',
      'Needs fingerprint m_flow_split.volume_ix_patterns; else NaN (never fires).',
      'Windowed form also needs window_size_sec on m_flow_split_window.',
    ].join('\n'),
  },
  vol_sell: {
    title: 'vol_sell — volume-side sells (SOL)',
    body: [
      'SOL from sells classified as volume-side (same classifier as vol_buy).',
      '',
      'Examples:',
      '  >1     volume wallets dumping',
      '  >3 on exit → sell when tooling exits hard',
      '',
      'Unconfigured fingerprint ⇒ NaN. Windowed form needs window_size_sec.',
    ].join('\n'),
  },
  vol_net: {
    title: 'vol_net — volume-side net (SOL)',
    body: [
      'vol_buy − vol_sell. Positive = net volume-side buying; negative = net dumping.',
      '',
      'Examples:',
      '  >1     tooling still accumulating',
      '  <0     volume-side net selling',
      '',
      'Not monotonic. Unconfigured fingerprint ⇒ NaN.',
    ].join('\n'),
  },
  vol_gross: {
    title: 'vol_gross — volume-side activity (SOL)',
    body: [
      'vol_buy + vol_sell — how much volume-side tape traded (direction ignored).',
      '',
      'Example: >5 → at least 5◎ of volume-side flow (buys+sells).',
      '',
      'Useful with vol_share to require both activity and dominance.',
    ].join('\n'),
  },
  nonvol_buy: {
    title: 'nonvol_buy — organic buys (SOL)',
    body: [
      'SOL spent on buys that are NOT volume-side (organic / retail tape).',
      '',
      'Example: >2 → real buy interest outside tooling wallets.',
      '',
      'Trades with missing ix_labels count as organic unless wallet-tagged/creator.',
      'Unconfigured fingerprint ⇒ NaN.',
    ].join('\n'),
  },
  nonvol_sell: {
    title: 'nonvol_sell — organic sells (SOL)',
    body: [
      'SOL from organic (non–volume-side) sells.',
      '',
      'Example: >2 on exit → organic holders dumping.',
      '',
      'Unconfigured fingerprint ⇒ NaN.',
    ].join('\n'),
  },
  nonvol_net: {
    title: 'nonvol_net — organic net (SOL)',
    body: [
      'nonvol_buy − nonvol_sell. Positive = organic accumulation; negative = organic exit.',
      '',
      'Examples:',
      '  >1     organic net buying',
      '  <0     organic net selling',
      '',
      'Not monotonic. Unconfigured fingerprint ⇒ NaN.',
    ].join('\n'),
  },
  nonvol_gross: {
    title: 'nonvol_gross — organic activity (SOL)',
    body: [
      'nonvol_buy + nonvol_sell — total organic tape (direction ignored).',
      '',
      'Example: >3 → meaningful organic churn alongside (or instead of) volume-side flow.',
    ].join('\n'),
  },
  vol_share: {
    title: 'vol_share — volume share of tape (%)',
    body: [
      'vol_gross / (vol_gross + nonvol_gross) × 100. How much of total flow is volume-side.',
      'NaN when total gross is 0 (no scored flow yet).',
      '',
      'Examples:',
      '  >70     tape dominated by volume-side',
      '  <30     mostly organic',
      '  40..60  mixed / contested',
      '',
      'Unit is percent (not SOL). Unconfigured fingerprint ⇒ NaN.',
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
    title: 'window_size_sec — trailing lookback',
    body: [
      'How many seconds of recent trades to include for dynamic groups:',
      '  • m_flow_window — gross_flow, net_flow, buy, sell',
      '  • m_flow_split_window — vol_*, nonvol_*, vol_share (same split as lifetime)',
      '',
      'Required if any metric in that group has a condition. Example: 10 → last 10s only.',
      '',
      'All windowed axes on the same side must share one window per group',
      '(sweep rejects conflicting windows). Lifetime m_flow_split ignores this.',
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
      'Locked after create — unlock the padlock to flip paper↔real. That only affects future entries; open positions keep their original mode. Sizing/caps stay editable while live; fingerprint + entry/exit conditions lock once active.',
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
      'Raw strategy params: take_profit, stop_loss, entry{…}, exit{…}, reentry{…}.',
      '',
      'Each metric is a list of {operator, value} (one AND arm) or a nested list of arms for OR.',
      'Example liquidity outside band: [[{"operator":"<","value":30}],[{"operator":">=","value":70}]].',
      '',
      'Validated against the metric registry on save. Prefer the Builder tab unless you know the shape.',
    ].join('\n'),
  },
  reentry: {
    title: 'Re-entry',
    body: [
      'Off ⇒ one-shot: once a token+rule closes (Done), it never re-enters that token.',
      '',
      'On ⇒ after each NORMAL exit (TP / SL / an exit metric — never a dead/manual/migrated',
      'close) the rule waits the cooldown, then re-arms and can enter the same token again,',
      'up to the episode cap.',
      '',
      'This is what the dip-scalper needs: its edge is rapid re-entry, not a single trade.',
    ].join('\n'),
  },
  reentryCooldown: {
    title: 'Cooldown (seconds)',
    body: [
      'How long to wait after a close before the rule can re-arm the same token.',
      '',
      'A floor, not a timer: re-arm happens on the next trade/tick once the cooldown has',
      'elapsed. 0 ⇒ eligible on the very next event. Must be ≥ 0.',
    ].join('\n'),
  },
  reentryMaxEpisodes: {
    title: 'Max episodes per token',
    body: [
      'Hard cap on how many times this rule may enter a single token (across the whole run).',
      '',
      'The Nth close re-arms only while the episode count is below this. Integer ≥ 1.',
      'Note this also becomes what the rule’s max-total cap counts (episodes, not tokens).',
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
  volume_ix_patterns: {
    title: 'Volume-side ix patterns',
    body: [
      'Ordered instruction-label sequences that mark a trade as volume-side for',
      'm_flow_split / m_flow_split_window. Exact ordered match — same vocabulary as fingerprint ix_labels.',
      '',
      'Classifier (any one is enough):',
      '  1) trade ix_labels hash ∈ one of these pattern rows',
      '  2) wallet already tagged volume-side on THIS token (contagion)',
      '  3) creator wallet (always volume-side)',
      'Otherwise the trade is organic (nonvol_*).',
      '',
      'Example row: ["Pump.Fun: Buy","Token Program: CloseAccount"]',
      '',
      'Empty / missing m_flow_split key ⇒ every flow metric is NaN (conditions never fire).',
      'Discover candidates on Lab → Flow discovery, then Apply back here.',
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
    title: 'Token cap (corpus)',
    body: [
      'Max tokens loaded into the sweep corpus (RAM/time guard).',
      '',
      'The lake keeps the newest N non-mayhem tokens in the date range',
      '(ORDER BY created_at DESC). Older mints are not scored — even if they',
      'share a fingerprint with a group you are looking at.',
      '',
      'Server ceiling is 100 000; simulate has no such cap (known asymmetry).',
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
      'Trailing window for dynamic metrics on this axis (same as window_size_sec on a rule):',
      'm_flow_window and m_flow_split_window.',
      '',
      'Every windowed axis of the same group on the same side must share one window —',
      'conflicts are rejected. Lifetime / static metrics ignore this.',
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

// ── Flow discovery (lab) ─────────────────────────────────────────────────────

/** Form fields unique to Flow discovery (reuses SWEEP_FIELD_HELP for shared knobs). */
export const DISCOVERY_FIELD_HELP = {
  createdRange: {
    title: 'Created range (UTC)',
    body: [
      'Only tokens whose created_at falls in this window enter discovery.',
      '',
      'Leave either side empty for an open bound. Times are UTC (datetime-local → ISO).',
      'Example: last 24h of launches to rank volume-like ix structures.',
    ].join('\n'),
  },
  seedFingerprint: {
    title: 'Scope by saved fingerprint',
    body: [
      'When set, discovery scores only tokens that MATCH this fingerprint',
      '(engine match SSOT — same buckets / axes as live).',
      '',
      'UI then uses one “ALL” group; Apply writes volume_ix_patterns back to this fingerprint.',
      'Leave empty to partition manually with group-by / filters below.',
    ].join('\n'),
  },
  applyFingerprint: {
    title: 'Apply to fingerprint',
    body: [
      'Target fingerprint that receives the draft volume_ix_patterns on Apply.',
      '',
      '• Pick an existing row → PUT metric_config.m_flow_split on that fingerprint.',
      '• Empty → create / bind a fingerprint from the selected group key, then write patterns.',
      '',
      'Auto-match highlights a saved fingerprint whose axes already equal this group.',
    ].join('\n'),
  },
  draftPatterns: {
    title: 'Draft volume_ix_patterns',
    body: [
      'Checked structures from the table become pattern rows (exact ix_labels sequences).',
      'Edit freely, then Apply to write m_flow_split.volume_ix_patterns on the target fingerprint.',
      '',
      'Those patterns drive vol_* / nonvol_* / vol_share on rules that use this fingerprint.',
      'Empty draft cannot Apply — toggle at least one structure (or add a row in the editor).',
    ].join('\n'),
  },
  volumeSplit: {
    title: 'Flow split — checked structures',
    body: [
      'Live preview of the group’s scored SOL split into two buckets based on the checkboxes',
      'below: "Volume" = every row you’ve checked (would-be volume_ix_patterns); "Organic" =',
      'every unchecked row. Nothing here is saved — it’s just a preview of what Apply would',
      'flag as volume once you toggle rows, computed client-side from each row’s Gross◎.',
      '',
      'Example: group scored 100 SOL total; you check two rows worth 62 SOL combined →',
      'bar shows 62% volume / 38% organic. Only structures currently listed in the table',
      '(top 64 by rank) count toward the total.',
    ].join('\n'),
  },
} as const satisfies Record<string, HelpTip>;

/** Column tips for the discovery structure ranking table. */
export const DISCOVERY_COL_HELP = {
  vol: {
    title: 'Vol — include in draft',
    body: [
      'Check this box to add the row’s exact ix_labels sequence to the draft',
      'volume_ix_patterns list below the table. Unchecked rows are ignored on Apply —',
      'only checked structures get written to the fingerprint.',
      '',
      'Example: check the top-ranked ["Pump.Fun: Create","Pump.Fun: Buy"] row to mark',
      'that combo as manufactured volume; leave a low-lift ["Pump.Fun: Buy"] row unchecked',
      'to keep treating it as organic.',
    ].join('\n'),
  },
  structure: {
    title: 'Structure — trade ix_labels',
    body: [
      'The exact, ORDERED list of instruction labels seen on trades that used this pattern —',
      'same vocabulary as a fingerprint’s ix_labels field. Order matters: ["Create","Buy"] is',
      'a different structure from ["Buy","Create"].',
      '',
      'Example: ["Pump.Fun: Create","Pump.Fun: Buy"] means every trade counted in this row',
      'began with a Create instruction immediately followed by a Buy in the same transaction —',
      'a common bundler/sniper shape, not an organic manual buy.',
    ].join('\n'),
  },
  side: {
    title: 'Side — which trade direction uses this shape?',
    body: [
      '"buy-only" / "sell-only": this exact ix_labels sequence only ever shows up on one',
      'side — true for most venues, since Pump.Fun\'s buy and sell instructions have',
      'different labels already.',
      '',
      '"both sides": the same label sequence is used for BOTH buys and sells (e.g. Axiom’s',
      'generic swap instruction). Wash for a "both sides" row is meaningful on its own —',
      'the buy and its matching sell live in the same row, so a near-zero Wash really does',
      'mean a round-trip through this one shape.',
      '',
      'For "buy-only"/"sell-only" rows, Wash will almost always read ≈1 (there’s nothing',
      'in THIS row to net against) — that does NOT mean the wallet isn’t washing. It may',
      'just be washing through a different shape on the other side. Check Contagion% to see',
      'if that’s already covered.',
    ].join('\n'),
  },
  lift: {
    title: 'Lift × — is this shape special to this group?',
    body: [
      '(this shape’s % of THIS group’s SOL) ÷ (this shape’s % of ALL tokens’ SOL).',
      '',
      'Example: shape is 40% of this group’s SOL, but only 20% of all tokens’ SOL →',
      'Lift = 40 ÷ 20 = 2.0 — twice as concentrated here as everywhere else.',
      '',
      '≈ 1  → common everywhere, not distinctive to this group → ignore it.',
      '» 1  → tied to this group’s specific tooling → worth a closer look.',
      '(The "ambig" chip fires when even the TOP row’s lift < 1.25 — nothing here stands out.)',
    ].join('\n'),
  },
  share: {
    title: 'Share% — how big a slice of this group’s volume?',
    body: [
      'This shape’s SOL ÷ the group’s total SOL × 100.',
      '',
      'Example: group moved 100 SOL total, this shape accounts for 35 of it → Share% = 35.',
      '',
      'Size alone isn’t suspicious — a shape can be huge just because it’s the only common',
      'way to trade these tokens (Lift will then be ≈1 too, since it’s common everywhere).',
      'High Share% + high Lift together is the real signal.',
    ].join('\n'),
  },
  wash: {
    title: 'Wash 0–1 — do the buys and sells cancel out?',
    body: [
      'Average of |buys − sells| ÷ total volume, per token that used this shape.',
      '',
      'Example: a token bought 2.0 SOL and sold 1.9 SOL back on this shape →',
      '|2.0 − 1.9| ÷ 3.9 ≈ 0.03.',
      '',
      '→ 0  money went in a circle (buy then sell back ~the same amount) — classic bot',
      '     round-trip, it doesn’t actually want to hold the token.',
      '→ 1  one-sided (mostly buys, no matching sell) — looks like a real directional trade.',
    ].join('\n'),
  },
  recur: {
    title: 'Recur% — does this shape repeat across tokens?',
    body: [
      '% of the group’s tokens where this shape moved ≥ 0.05 SOL (the "meaningful volume"',
      'floor — smaller amounts are ignored as noise).',
      '',
      'Example: group has 20 tokens, this shape shows up meaningfully on 15 of them →',
      'Recur% = 75.',
      '',
      'High → the same script is being replayed token after token (reused tooling), not a',
      'one-off. Low → only happened on a couple of tokens — could just be coincidence.',
    ].join('\n'),
  },
  burst: {
    title: 'Burst% — are trades firing in tight clusters?',
    body: [
      '% of this shape’s trades with another trade of the SAME shape within ±1 Solana slot',
      '(~400ms).',
      '',
      'Example: 10 trades on this shape, 8 of them have a sibling trade within 1 slot →',
      'Burst% = 80.',
      '',
      'High → rapid-fire clusters landing almost simultaneously — automation, not a human',
      'clicking buttons. Low → spread out over time, more consistent with organic activity.',
    ].join('\n'),
  },
  reuse: {
    title: 'Reuse 0–1 — wallet concentration',
    body: [
      '1 − (distinct wallets ÷ number of trades) using this structure — rises toward 1 as',
      'fewer unique wallets account for more of the trades.',
      '',
      'High → a small set of wallets repeatedly fires this exact structure (classic',
      'multi-wallet bot farm). 0 → every trade came from a different wallet (no reuse).',
      '',
      'Example: 10 trades on this structure from only 2 distinct wallets →',
      '1 − (2 / 10) = 0.8.',
    ].join('\n'),
  },
  gross: {
    title: 'Gross◎ — structure volume',
    body: [
      'Total absolute SOL notional (buys + sells added together, NOT netted) that this',
      'structure moved inside the current group.',
      '',
      'Ranking context only — this number is never written to the fingerprint, only the',
      'ix_labels pattern is (via the Vol checkbox).',
      '',
      'Example: 5 buys of 1 SOL + 5 matching sells of 1 SOL on this structure → Gross◎ = 10,',
      'even though the net flow is ≈ 0 (that near-zero net is what Wash also measures).',
    ].join('\n'),
  },
  contagion: {
    title: 'Contagion% — would this row already get swept in?',
    body: [
      'Of THIS row’s SOL, how much comes from wallets that ALSO traded on a structure',
      'you’ve already checked above. Mirrors how the live engine actually classifies volume:',
      'once a wallet’s trade matches a checked pattern, ALL of that wallet’s later trades —',
      'any side, any shape — count as volume too, not just the matching row.',
      '',
      'Example: you check a buy-only row traded by wallets A, B, C. This sell-only row is',
      'traded 8 SOL by A and 2 SOL by a new wallet D → Contagion% = 8 / 10 = 80.',
      '',
      'High → checking the row above already covers this one live; you probably don’t need',
      'to check this row too. Low/blank → this row’s wallets aren’t caught by your current',
      'picks — check it separately if it’s part of the same wash flow (e.g. the sell leg of',
      'a buy-only shape you already checked). "—" = nothing checked yet, or this row is',
      'itself checked.',
    ].join('\n'),
  },
  suggested: {
    title: 'Auto — is this shape auto-flagged as volume?',
    body: [
      'A client-side verdict that composites the bot-likelihood columns into one call, so',
      'you don’t have to eyeball every row. A row is Suggested when it clears two gates and',
      'then trips at least TWO strong signals:',
      '',
      'Gates (either one blocks it): Lift × ≥ 1.25 (must be concentrated here, not ambient',
      'everywhere) AND Gross◎ ≥ 0.05 (ignore dust-sized shapes).',
      '',
      'Signals, each "strong" at ≥ 0.5 normalized: Recur% ≥ 50, Burst% ≥ 50, Reuse ≥ 0.5,',
      'and — for "both sides" rows only — Wash ≤ 0.5 (round-trip). For one-sided rows Wash',
      'is meaningless, so Contagion% ≥ 50 stands in for it (once you’ve checked something to',
      'compare against).',
      '',
      'The % shown is the mean of the applicable signals (0–100). A warning badge = Suggested',
      '(≥ 2 strong); a dim % = scored but under the bar; "—" = gated out (low Lift or dust).',
      'Suggestions are advisory — "Auto-select suggested" checks them all at once, and you',
      'can still toggle any row by hand.',
    ].join('\n'),
  },
} as const satisfies Record<string, HelpTip>;
