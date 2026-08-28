/** Plain-language ⓘ help for rule / fingerprint / sweep authoring.
 *
 *  Wording goal: detailed enough to use without reading the plan docs, still
 *  scannable. Bodies may use newlines (`whitespace-pre-line` in InfoTooltip).
 *  Mirrors engine semantics: entry AND / exit OR across metrics; within one
 *  metric `,` = AND, `|` = OR, `lo..hi` = inclusive range. */

export interface HelpTip {
  title: string;
  body: string;
  /** Optional ASCII figure (mono, `whitespace-pre`) drawn above the body. */
  figure?: string;
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
      'Monotonic tip: time and m_flow_lifetime buy/sell/gross_flow only go up. An entry',
      'like time < 30 (or lifetime gross_flow < 5) can never succeed after it is crossed —',
      'the engine disarms that arm (derived unsatisfiable).',
    ].join('\n'),
  },
  exit: {
    title: 'Exit side — when to SELL',
    body: [
      'Checked while you hold a position, alongside take-profit and stop-loss.',
      '',
      'Prefer the TP / SL % fields for classic %-from-entry exits (labeled TakeProfit /',
      'StopLoss). Those are sugar for m_position.pnl — use the pnl metric only for',
      'extra/custom bounds (e.g. a catastrophe stop beside SL).',
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
  m_state: {
    title: 'm_state — point-in-time facts',
    body: [
      'Always-on metrics that do not need a trailing window.',
      '',
      '• time — seconds since token creation (age).',
      '• liquidity — current pool SOL reserves.',
      '• ix_count — instructions in the CREATION transaction (a launch-tooling tell).',
      '• prior_launches — tokens this creator launched BEFORE this one.',
      '',
      'The last two are creation-time facts: static from birth, so a gate on either is',
      'a token filter that can never re-trigger. Both read NaN when unknown, so an',
      'unknown launch stays unmatched rather than counting as 0.',
      '',
      'Use for age gates, pool-depth filters and creator / tooling screens. Kind: static.',
    ].join('\n'),
  },
  m_price_lifetime: {
    title: 'm_price_lifetime — price behaviour',
    body: [
      'How price has been moving since the token (and your hold) has been alive.',
      '',
      '• stall — seconds since the price last set a new all-time high (how long since progress).',
      '• trail — % drop from the lifetime peak (give-back from the high).',
      '• rise — % climb from the lifetime trough (bounce off the all-time low).',
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
  m_flow_lifetime: {
    title: 'm_flow_lifetime — lifetime flow',
    body: [
      'Sums buys/sells since token birth (no trailing window, no classifier).',
      '',
      'Metrics (all SOL):',
      '• gross_flow — buys + sells (total activity ever)',
      '• net_flow — buys − sells (lifetime direction)',
      '• buy — buys only',
      '• sell — sells only',
      '',
      'Use as a maturity / critical-mass gate. For "hot right now", prefer',
      'm_flow_window. No window_size_sec. Kind: static.',
    ].join('\n'),
  },
  m_flow_window: {
    title: 'm_flow_window — trailing flow',
    body: [
      'Sums buys/sells over a trailing window. Unlike m_flow_lifetime (totals since',
      'birth), the lookback rolls forward.',
      '',
      'Metrics:',
      '• gross_flow — buys + sells, SOL (activity)',
      '• net_flow — buys − sells, SOL (direction)',
      '• buy / sell — one side only, SOL',
      '• buy_share — buy / (buy + sell) in PERCENT 0-100, not 0-1: direction',
      '  independent of size. NaN on an empty window, so it never fires on silence.',
      '• trade_count — how many trades landed (a count). One wallet re-entering ten',
      '  times reads 10 here and 1 in m_crowd_window.unique_wallets.',
      '• buy_count / sell_count — how many BUYS, how many SELLS. trade_count counts',
      '  both, so on a one-slot window only these answer "who bought the slice". The',
      '  two always add up to trade_count; a condition cannot subtract, which is why',
      '  sell_count exists at all.',
      '• trade_share / sol_share — what fraction of the window landed in a SLICE',
      '  nested inside it (set slice_size_sec). trade_share counts trades, sol_share',
      '  counts SOL, and they are not the same reading: ten prints of 0.1 SOL and one',
      '  print of 10 are identical in the first and far apart in the second. On a',
      '  PRINT window only sol_share still varies. Entry side only, for now.',
      '',
      'If you use any metric here you must set EXACTLY ONE size — window_size_sec',
      '(e.g. 10), window_size_slots (e.g. 30) or window_size_prints (e.g. 20) — plus',
      'an optional window_lag. slice_size_* is required by trade_share / sol_share and',
      'REJECTED on a row without them: a slice nothing reads is a gate that does nothing.',
      'Kind: dynamic.',
    ].join('\n'),
  },
  m_crowd_window: {
    title: 'm_crowd_window — who is trading, over a window',
    body: [
      'The two readings about PEOPLE rather than money, over the same trailing window',
      'm_flow_window uses.',
      '',
      'Metrics:',
      '• unique_wallets — how many distinct wallets traded (a count). One wallet',
      '  churning and a crowd arriving are identical in gross_flow and far apart here.',
      '• trades_per_wallet — trade_count / unique_wallets. Low is a crowd arriving,',
      '  high is one wallet working the tape. A COUNT ratio, never an identity, so',
      '  wallet rotation does not defeat it. NaN on an empty window, never 0 — a 0',
      '  would let "trades_per_wallet <= 2" pass on a dead tape.',
      '',
      'Why its own group rather than part of m_flow_window: these two are the only',
      'metrics that need the WALLET column loaded. An offline read without it folds',
      'every trade as one anonymous wallet, so unique_wallets reads 1 forever and the',
      'gate looks strict instead of broken. One group, one load obligation.',
      '',
      'To gate on flow AND crowd over one window, author both groups at the same',
      'window_size_sec. They are ANDed, and they share one buffer.',
      'Kind: dynamic.',
    ].join('\n'),
  },
  m_flow_ix: {
    title: 'm_flow_ix — flow split by instruction structure (lifetime)',
    body: [
      'Splits every trade by whether its instruction shape is TAGGED by the',
      'fingerprint\'s ix_patterns (+ wallet contagion + creator wallet). Tagged is',
      'usually creator tooling and untagged usually organic, but the metric reads the',
      'MATCH, not the motive, and is named for what it reads.',
      '',
      'Metrics (SOL unless noted): tagged_buy/sell/net/gross, untagged_*, tagged_share (%).',
      'Unconfigured fingerprint ⇒ all NaN (conditions never fire). Kind: static.',
    ].join('\n'),
  },
  m_flow_ix_window: {
    title: 'm_flow_ix_window — flow split by instruction structure (trailing)',
    body: [
      'Same split as m_flow_ix, but over a trailing window (window_size_sec,',
      'window_size_slots or window_size_prints).',
      'Reads the same ix_patterns from the fingerprint (no duplicate config).',
      '',
      'Metric names mirror m_flow_ix (tagged_*, untagged_*, tagged_share). Kind: dynamic.',
    ].join('\n'),
  },
  m_position: {
    title: 'm_position — your open position (EXIT ONLY)',
    body: [
      'Metrics anchored on YOUR entry fill — they only exist while you hold, so this',
      'group is exit-only (hidden on the entry side).',
      '',
      '• retrace — % below the highest price since entry (the trailing stop).',
      '• bounce — % above the lowest price since entry (recovery from the trough).',
      '• pnl — signed % vs entry (advanced). Prefer the TP / SL % fields for the classic',
      '  labeled exits; those desugar into pnl. Use this row for extra/custom bounds.',
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
  ix_count: {
    title: 'ix_count — instructions in the creation transaction',
    body: [
      'How many instructions the token’s CREATE transaction carried — a launch-tooling',
      'fingerprint reduced to one number. A plain launch is a handful; a bundled one,',
      'or one built by a launcher bot, is many.',
      '',
      'Static from birth: it never moves, so a gate on it is a TOKEN FILTER, not a',
      'timing signal, and an arm it disarms can never re-arm.',
      '',
      'Examples:',
      '  <=5      plain creations only — screen out elaborate tooling',
      '  >=12     only the heavily-bundled launches',
      '',
      'NaN when the creation labels are unknown, and NaN satisfies nothing — so an',
      'unknown launch is excluded by any ix_count gate rather than assumed simple.',
    ].join('\n'),
  },
  prior_launches: {
    title: 'prior_launches — the creator’s launches before this token',
    body: [
      'How many tokens this token’s creator launched BEFORE it. 0 = a first-time',
      'launcher; a large number = a serial one running a factory.',
      '',
      'Counted over a trailing window (30 days), not all of history — so the same',
      'creator reads the same number live and in a backtest. Static from birth: a',
      'token filter, never a timing signal.',
      '',
      'Examples:',
      '  =0        the creator’s first launch in the window',
      '  <=5       new-ish creators only',
      '  >=100     only factory output (usually the side to AVOID)',
      '',
      'NaN when the creator is unknown, so an unresolved launch is excluded rather',
      'than counted as a first one — which would quietly widen "=0" to everything.',
      '',
      'Reads the creator across OTHER tokens, so it needs the tokens table: available',
      'live and in Simulate, rejected by a grouped sweep (the lake corpus has no',
      'creator column and every cell would score on zero trades).',
    ].join('\n'),
  },
  buy_share: {
    title: 'buy_share — share of window SOL that is buys (percent)',
    body: [
      'buy / (buy + sell) over the trailing window, in PERCENT 0-100 — not 0-1. The',
      'DIRECTION of the tape, independent of its size.',
      '',
      'net_flow conflates the two: +5◎ net is a different situation on 6◎ of turnover',
      'than on 200◎. This reads high when one side is being ABSORBED rather than',
      'matched.',
      '',
      'Examples:',
      '  >=80      four out of five SOL traded is buying',
      '  >=95      near one-sided — nobody is selling into it',
      '  <=30      distribution',
      '',
      'NaN on an empty window (no flow, no direction to report), so it never fires on',
      'silence. Needs a window size (window_size_sec, _slots or _prints).',
    ].join('\n'),
  },
  unique_wallets: {
    title: 'unique_wallets — distinct traders in the window (count)',
    body: [
      'How many DIFFERENT people traded the token over the last N seconds — as against',
      'gross_flow’s how much SOL, and trade_count’s how many trades.',
      '',
      'One wallet churning and a crowd arriving look identical in SOL and different',
      'here. Pair the two to tell a real crowd from one bot: high gross_flow with low',
      'unique_wallets is wash volume. Both live in m_crowd_window.',
      '',
      'Examples:',
      '  >=10     a real crowd, not one bot',
      '  <=2      one or two wallets are the whole tape',
      '',
      'Needs a window size (window_size_sec, _slots or _prints). Wallet-keyed: an',
      'offline run loaded without wallet',
      'identity sees every trade as one anonymous wallet, so this reads 1.',
    ].join('\n'),
  },
  trade_count: {
    title: 'trade_count — trades in the window (count)',
    body: [
      'How many trades landed over the last N seconds — how BUSY the tape is, as',
      'against m_crowd_window.unique_wallets’ how many people are on it.',
      '',
      'One wallet re-entering ten times reads 10 here and 1 there. Unlike',
      'm_crowd_window.unique_wallets this needs no wallet column, so it survives a load that',
      'did not ask for wallet identity.',
      '',
      'Examples:',
      '  >=8      an active tape',
      '  <=3      a quiet one — pairs with a high buy_share for "quiet accumulation"',
      '',
      'Needs a window size (window_size_sec, _slots or _prints).',
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
    title: 'rise — climb from the low (%)',
    body: [
      'Percent above the low-water mark. 0 at the low; grows as price recovers/breaks out.',
      '',
      'Two groups use this name:',
      '• m_price_lifetime.rise — vs the LIFETIME trough (bounce off the all-time low).',
      '• m_price_window.rise — vs the ROLLING window low (breakout/momentum).',
      '',
      'Examples:',
      '  >=15    (window) 15%+ above the recent low → momentum/breakout entry',
      '  <=20    (lifetime) still within 20% of the all-time low (near the floor)',
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
  bounce: {
    title: 'bounce — climb from since-entry low (%)  [exit only]',
    body: [
      'Percent above the lowest price seen SINCE YOUR ENTRY — recovery off the',
      'post-entry trough. At entry the trough is your fill, so before any dip it equals',
      'pnl; after a dip+recovery it measures the bounce from the worst since-entry print.',
      '',
      'Example:  >=15   sell once price has bounced 15% off the since-entry low.',
    ].join('\n'),
  },
  arm_above_pct: {
    title: 'arm ≥ % — disarm the trail until you\'re this far in profit',
    body: [
      'retrace/bounce measure from the since-entry peak/trough, which starts AT your',
      'entry fill. Unarmed, that makes retrace a hard stop from entry — it fires on the',
      'normal dip you bought into, before any real run-up.',
      '',
      'Setting arm ≥ N%  disables retrace/bounce until pnl has reached N% — only then',
      'does the trail start watching for a pullback off the real peak. 0 = arm at',
      'break-even. Leave blank = unarmed (today\'s default, usually wrong for a dip-buy',
      'entry).',
    ].join('\n'),
  },
  pnl: {
    title: 'pnl — profit/loss vs entry (%)  [exit only, advanced]',
    body: [
      'Signed percent vs your entry price. Positive = in profit, negative = underwater.',
      '',
      'Prefer the TP / SL % fields for the usual exits — they are sugar for this metric',
      '(TP → pnl >= tp, SL → pnl <= −sl) and keep the TakeProfit / StopLoss labels.',
      'Use this row only for extra or non-sugar bounds.',
      '',
      'Examples:',
      '  <=-25   −25% catastrophe stop beside a normal SL',
      '  >=80    custom profit bound when you intentionally skip the TP field',
      '',
      'Do not restate the same bound as TP/SL here — the editor blocks that duplicate.',
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
      'Buys + sells in SOL. Measures how much traded, not who won.',
      '',
      'Two groups use this name:',
      '• m_flow_lifetime.gross_flow — since token birth (maturity / critical-mass gate).',
      '• m_flow_window.gross_flow — over window_size_sec (hot-right-now filter).',
      '',
      'Example: >5 with window 10 → more than 5◎ changed hands in the last 10 seconds.',
    ].join('\n'),
  },
  net_flow: {
    title: 'net_flow — buy pressure (SOL)',
    body: [
      'Buys minus sells in SOL. Positive = net buying; negative = net selling.',
      '',
      'Two groups use this name:',
      '• m_flow_lifetime.net_flow — lifetime direction since birth.',
      '• m_flow_window.net_flow — over window_size_sec (short-horizon pressure).',
      '',
      'Examples:',
      '  >2      strong net buying in the window',
      '  <0      net selling',
    ].join('\n'),
  },
  buy: {
    title: 'buy — buy volume (SOL)',
    body: [
      'SOL spent on buys only (sells ignored).',
      '',
      'Two groups use this name:',
      '• m_flow_lifetime.buy — since token birth.',
      '• m_flow_window.buy — inside the trailing window (needs window_size_sec).',
      '',
      'Example: >3 → at least 3◎ of buys in the chosen lookback.',
    ].join('\n'),
  },
  sell: {
    title: 'sell — sell volume (SOL)',
    body: [
      'SOL from sells only (buys ignored).',
      '',
      'Two groups use this name:',
      '• m_flow_lifetime.sell — since token birth.',
      '• m_flow_window.sell — inside the trailing window (needs window_size_sec).',
      '',
      'Example: >2 → heavy selling in the lookback — often an exit signal.',
    ].join('\n'),
  },

  // m_flow_ix / m_flow_ix_window — same JSON names; registry appends unit/tol/monotonic.
  tagged_buy: {
    title: 'tagged_buy — buys the fingerprint tags (SOL)',
    body: [
      'SOL spent on buys the classifier TAGS. Usually creator tooling / wash, but the',
      'metric reads the tag, not the motive — so gate on it for what it measures.',
      '',
      'A trade is tagged if: its ix_labels match an ix_patterns row, OR its wallet was',
      'already tagged on this token (contagion), OR it is the creator.',
      '',
      'Examples:',
      '  >2     heavy tagged buying (lifetime or window)',
      '  <0.5   little tooling buy pressure',
      '',
      'Needs fingerprint m_flow_ix.ix_patterns; else NaN (never fires).',
      'Windowed form also needs window_size_sec on m_flow_ix_window.',
    ].join('\n'),
  },
  tagged_sell: {
    title: 'tagged_sell — sells from tagged wallets (SOL)',
    body: [
      'SOL from sells the classifier tags (same classifier as tagged_buy).',
      '',
      'Examples:',
      '  >1     tagged wallets dumping',
      '  >3 on exit → sell when tooling exits hard',
      '',
      'Unconfigured fingerprint ⇒ NaN. Windowed form needs window_size_sec.',
    ].join('\n'),
  },
  tagged_net: {
    title: 'tagged_net — tagged buy minus sell (SOL)',
    body: [
      'tagged_buy − tagged_sell. Positive = tagged wallets accumulating; negative =',
      'tagged wallets distributing.',
      '',
      'Examples:',
      '  >1     tooling still accumulating',
      '  <0     tagged net selling',
      '',
      'Not monotonic. Unconfigured fingerprint ⇒ NaN.',
    ].join('\n'),
  },
  tagged_gross: {
    title: 'tagged_gross — tagged activity (SOL)',
    body: [
      'tagged_buy + tagged_sell — how much tagged tape traded (direction ignored).',
      '',
      'Example: >5 → at least 5◎ of tagged flow (buys+sells).',
      '',
      'Useful with tagged_share to require both activity and dominance.',
    ].join('\n'),
  },
  untagged_buy: {
    title: 'untagged_buy — buys the fingerprint does not tag (SOL)',
    body: [
      'SOL spent on buys the classifier does NOT tag — the rest of the tape, usually',
      'organic / retail.',
      '',
      'Example: >2 → real buy interest outside the tagged wallets.',
      '',
      'Trades with missing ix_labels are untagged unless the wallet is already tagged',
      'or is the creator. Unconfigured fingerprint ⇒ NaN.',
    ].join('\n'),
  },
  untagged_sell: {
    title: 'untagged_sell — sells from untagged wallets (SOL)',
    body: [
      'SOL from sells the classifier does not tag.',
      '',
      'Example: >2 on exit → untagged holders dumping.',
      '',
      'Unconfigured fingerprint ⇒ NaN.',
    ].join('\n'),
  },
  untagged_net: {
    title: 'untagged_net — untagged buy minus sell (SOL)',
    body: [
      'untagged_buy − untagged_sell. Positive = untagged accumulation; negative =',
      'untagged exit.',
      '',
      'Examples:',
      '  >1     untagged net buying',
      '  <0     untagged net selling',
      '',
      'Not monotonic. Unconfigured fingerprint ⇒ NaN.',
    ].join('\n'),
  },
  untagged_gross: {
    title: 'untagged_gross — untagged activity (SOL)',
    body: [
      'untagged_buy + untagged_sell — total untagged tape (direction ignored).',
      '',
      'Example: >3 → meaningful untagged churn alongside (or instead of) tagged flow.',
    ].join('\n'),
  },
  tagged_share: {
    title: 'tagged_share — tagged share of tape (%)',
    body: [
      'tagged_gross / (tagged_gross + untagged_gross) × 100. How much of total flow the',
      'fingerprint tags.',
      'NaN when total gross is 0 (no scored flow yet).',
      '',
      'Examples:',
      '  >70     tape dominated by tagged flow',
      '  <30     mostly untagged',
      '  40..60  mixed / contested',
      '',
      'Unit is percent (not SOL). Unconfigured fingerprint ⇒ NaN.',
    ].join('\n'),
  },
};

/**
 * The tooltip body for one metric: its **registry definition** first, then the unit /
 * `=` tolerance / monotonic facts derived from the same spec, then whatever extended
 * prose {@link METRIC_HELP} adds.
 *
 * The definition comes from `spec.description` — authored once on the backend
 * `MetricSpec`, beside the metric it defines — so a tooltip cannot say something the
 * engine does not. `METRIC_HELP` is **extended guidance only**: worked examples, reading
 * guides, refutations. It must never restate the definition, and a metric with no entry
 * there is fully documented by the registry alone.
 */
export function metricHelpBody(
  metric: string,
  spec?: { unit: string; eq_tolerance: number; monotonic: boolean; description?: string },
): string {
  const extended = METRIC_HELP[metric]?.body;
  const base =
    spec?.description ??
    extended ??
    'Registry metric — type a condition in the box, or leave empty to ignore.';
  if (!spec) return base;
  // Every registry unit, named. This used to be a two-branch ternary falling through
  // to 'SOL', which documented every `count` metric (`unique_wallets`, `trade_count`,
  // `ix_count`, `prior_launches`) as SOL and told the reader their `=` bucket was
  // "+/-0.25 SOL". A metric added with a new unit should read wrong loudly, not
  // silently become SOL, hence the explicit map plus a passthrough default.
  const UNIT_NAME: Record<string, string> = {
    seconds: 'seconds',
    percent: 'percent',
    sol: 'SOL',
    count: '',
  };
  const unit = UNIT_NAME[spec.unit] ?? spec.unit;
  const amount = (n: number) => (unit ? `${n} ${unit}` : String(n));
  const half = spec.eq_tolerance / 2;
  const bits = [
    '',
    `Unit: ${unit || 'a plain count'}.`,
    `= / != bucket: values within ±${amount(half)} of the target count as equal (tol ${spec.eq_tolerance}).`,
  ];
  if (spec.monotonic) {
    bits.push(
      'Monotonic: value never decreases. An entry upper bound (e.g. time < 30) that is crossed permanently disarms the arm.',
    );
  }
  // Extended prose goes BELOW the definition and the facts, and only when it is not
  // already the base (a registry payload with no description falls back to it).
  const tail = extended && extended !== base ? `\n\n${extended}` : '';
  return `${base}${bits.join('\n')}${tail}`;
}

// ── Strict params ────────────────────────────────────────────────────────────

export const STRICT_PARAM_HELP: Record<string, HelpTip> = {
  slice_size_sec: {
    title: 'slice_size_sec — the recent slice',
    body: [
      "m_flow_window's second window: the SHORT one, nested inside the reference span.",
      'On a slot row the twin param slice_size_slots takes its place — both axes of',
      'the group count in the same unit.',
      '',
      'trade_share = trades in the last slice_size_sec, as a percent of trades in',
      'the last window_size_sec. It measures how CONCENTRATED the tape is in time,',
      'independent of how busy it is: ten trades in the last 3s and ten spread over',
      'a minute are the same trade_count and the same gross_flow, and 50 vs 10 here.',
      '',
      'Must be > 0 and <= window_size_sec. Reads 100 on a token younger than it —',
      'a true reading of a short life, not a maturity signal, so gate m_state.time',
      'yourself if that is what you mean.',
    ].join('\n'),
  },
  slice_size_slots: {
    title: 'slice_size_slots — the recent slice, in slots',
    body: [
      "m_flow_window's second window when the row counts in slots. Same quantity as",
      'slice_size_sec — trades in the slice as a percent of trades in the reference',
      'window — measured in the discrete buckets the chain actually batches in.',
      '',
      'Both axes of the group must use the SAME unit: a slice in slots over a',
      'reference in seconds is a ratio across two different clocks, and the rule is',
      'rejected at save. Must be > 0 and <= window_size_slots.',
      '',
      'The canonical shape is slice 1 / window 30: the current slot against the',
      'half-minute of tape behind it.',
    ].join('\n'),
  },
  slice_size_prints: {
    title: 'slice_size_prints — the recent slice, in prints',
    body: [
      "m_flow_window's second window when the row counts in prints. Same quantity as",
      'slice_size_sec — trades in the slice as a percent of trades in the reference',
      'window — measured in the token\u2019s own transactions.',
      '',
      'Note what that makes trade_share on this basis: a count over a count, so it is',
      'the constant 100 * slice / window on every tape. It carries no information.',
      'Its SOL twin sol_share is the reading that survives here: the same two spans,',
      'but the numerator is money, which still varies when the counts cannot.',
      '',
      'Both axes of the group must use the SAME unit, and slice <= window.',
    ].join('\n'),
  },
  window_size_sec: {
    title: 'window_size_sec — trailing lookback, in seconds',
    body: [
      'How many seconds of recent trades to include for dynamic groups:',
      '  • m_flow_window — gross_flow, net_flow, buy, sell, trade_count, buy_count,',
      '    sell_count, buy_share, and the nested-slice pair trade_share / sol_share',
      '  • m_crowd_window — unique_wallets, trades_per_wallet',
      '  • m_flow_ix_window — tagged_*, untagged_*, tagged_share (same split as lifetime)',
      '  • m_price_window — trail, rise',
      '',
      'Every dynamic group needs EXACTLY ONE size — this, window_size_slots or',
      'window_size_prints; never two and never none. The window is the closed interval',
      '[now − lag − size, now − lag]. Example: 10 → the last 10s.',
      '',
      'Each dynamic group instance has its own window: you can author the same',
      'group at several windows (e.g. m_flow_window at 30s and 60s); each',
      'distinct window becomes its own clause. Static groups (m_flow_lifetime,',
      'm_flow_ix, m_price_lifetime) take no window at all.',
    ].join('\n'),
  },
  window_size_slots: {
    title: 'window_size_slots — trailing lookback, in slots',
    body: [
      'The same lookback as window_size_sec, counted in SLOTS instead of seconds.',
      'Set one or the other, never both.',
      '',
      'Time is continuous and slots are discrete, and the difference is not a',
      'rounding detail. A slot is what the chain batches in, so a bundle is a slot',
      'fact and never a time fact: at ~400ms per slot a one-second window straddles',
      'two or three slots and merges bursts that landed separately.',
      '',
      'A slot window is exactly `size` slots — size 1, lag 0 is the current slot',
      'alone. Its cursor is the slot number the feed reports, never a time estimate,',
      'so it holds its last reading until a trade moves it.',
      '',
      'A load without the slot column cannot advance a slot window: an offline run',
      'that did not request slots reads such a metric as unreadable, which satisfies',
      'nothing.',
    ].join('\n'),
  },
  window_size_prints: {
    title: 'window_size_prints — trailing lookback, in PRINTS',
    body: [
      "The same lookback as window_size_sec, counted in this token's own",
      'transactions. Set one size param or another, never two.',
      '',
      'This is the only basis in which a quantity is a statement about a TRADE.',
      '"10 SOL in the last second" is ten one-SOL prints or one ten-SOL print, and no',
      'wall-clock or slot span can tell them apart; window_size_prints 1 with lag 0 is',
      'the current transaction alone, so gross_flow > 10 on it means exactly "this tx',
      'moved 10 SOL".',
      '',
      'A print window is exactly `size` prints. Its cursor moves ONLY on a trade, so',
      'silence never decays it: 20 prints back is 20 prints back whether they landed',
      'in one slot or over an hour. That is the property a seconds window cannot have',
      '— and the reason a print gate reads the same on a busy token and a dead one.',
      '',
      'A token younger than the window holds fewer prints than you asked for; the',
      'reading is over what exists, which is a true reading of a short life, so gate',
      'm_state.time yourself if you mean maturity.',
    ].join('\n'),
  },
  window_lag: {
    title: 'window_lag — how far back the window ENDS',
    body: [
      'How many units (seconds, slots or prints — matching the size param) back from',
      'now the window stops. 0 — the default — means it ends at now.',
      '',
      'This is what makes a window causal in its own terms. A gate on "the state',
      'entering this slot" must not be able to see the slot it is firing in, and',
      'lag 1 on a slot window is exactly that: the slice is size 1 / lag 0 and the',
      'quiet tape before it is size 30 / lag 1 — two spans that cannot leak into',
      'each other.',
      '',
      'The lag belongs to the GROUP, so both axes of a two-window group share it: a',
      'two-window group is two spans on one clock.',
      '',
      'A lagged window reads a different span from an unlagged one of the same size,',
      'so the two are different clauses and label themselves differently (30sl@1).',
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
  tags: {
    title: 'Tags',
    body: [
      'Free-form labels for slicing the Rules board — chip-filter to show only a family, or hide a batch you are not looking at right now.',
      '',
      'Presentational only: a tag never affects matching, arming, or PnL, and is not part of a rule’s trading identity (two rules that trade the same way still collide on the duplicate check however they are tagged). Hiding a rule by tag is NOT the same as Disable, which also blocks activation.',
      '',
      'Namespace with a colon to keep the set navigable — fam:scalper, src:sweep, stage:paper-test, risk:high. The server canonicalizes what you type (lowercase, dashes for spaces, deduped), so “Paper Test” and “paper_test” become one tag.',
      '',
      'Editable while the rule is live — a label is not a condition.',
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
      'While at the cap, new entries are blocked until something exits. Blank (∞) = unlimited — every matching token may be entered at once, so size the buy amount for that.',
    ].join('\n'),
  },
  maxTotal: {
    title: 'Max total entries',
    body: [
      'Lifetime number of entries allowed for the whole run of this rule.',
      '',
      'Blank (∞) = unlimited. After N successful entries, the rule stops taking new tokens.',
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
      'Primary control for a labeled TakeProfit exit. Sugar for m_position.pnl >= this %',
      '(same evaluation path as an authored pnl condition — not a second mechanism).',
      '',
      'Examples: 100 → +100% (2× entry). 50 → +50%.',
      '',
      'ORs with stop-loss and exit metrics — any one can close. Leave empty to disable TP.',
      'Prefer this field over writing pnl >= … in the exit metrics.',
    ].join('\n'),
  },
  scaleOut: {
    title: 'Scale-out (tranched exit)',
    body: [
      'Ordered partial exits: each stage sells a % of the INITIAL bag when its',
      'conditions (or stage TP) fire, then advances. Global exit / SL still close',
      '100% of whatever remains — catastrophe path is unchanged.',
      '',
      'Sell % is of the initial bag (not the remainder). Blank sell % = remainder',
      'stage (must be last): closes the stub under its own conditions (e.g. a',
      'tighter trail or a pure time-stop). At most 3 partial stages + optional',
      'remainder; sum of partials ≤ 99%.',
      '',
      'After the last partial, the position keeps the global exit side unless a',
      'remainder stage is authored.',
    ].join('\n'),
  },
  stopLoss: {
    title: 'Stop loss (%)',
    body: [
      'Primary control for a labeled StopLoss exit. Sugar for m_position.pnl <= −this %',
      '(same evaluation path as an authored pnl condition — not a second mechanism).',
      '',
      'Examples: 30 → −30% from entry. 50 → −50%.',
      '',
      'ORs with take-profit and exit metrics. Leave empty to disable SL.',
      'Prefer this field over writing pnl <= … in the exit metrics.',
    ].join('\n'),
  },
  paramsJson: {
    title: 'Params JSON',
    body: [
      'Raw strategy params: take_profit, stop_loss, entry{…}, exit{…}, scale_out[…], reentry{…}.',
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
  exclusive: {
    title: 'Exclusive',
    body: [
      'Off ⇒ today’s behavior: rules hold positions independently, so several rules can',
      'stack on the same token at once.',
      '',
      'On ⇒ skip entry while ANY other rule already holds this token — including an',
      'in-flight buy or sell, and including a manual buy. Use it when rules are meant to',
      'compete for the same opportunity rather than stack on it.',
      '',
      'Blocked is not disarmed: the rule stays armed and retries once the holder lets go.',
      'Asymmetric by design — a non-exclusive rule never checks anyone, so it can still',
      'enter a token an exclusive rule holds.',
      '',
      'The grouped sweep IGNORES this — sweep numbers stay un-deconflicted upper bounds.',
    ].join('\n'),
  },
  exclusivePriority: {
    title: 'Priority',
    body: [
      'Higher priority wins when two exclusive rules would enter the same token at once.',
      'Default 0; ties break by rule ID.',
      '',
      'Only matters between two exclusive rules — it is not a general scheduling knob.',
    ].join('\n'),
  },
  buyPctOfVsol: {
    title: 'Buy % of vSOL',
    body: [
      'Size each buy as a percent of the pool’s SOL reserve at the entry instant,',
      'instead of the fixed buy amount above. Blank = keep the fixed amount.',
      '',
      'Why: our own price impact is exactly buy / vsol. A fixed size inside a liquidity',
      'band that spans 2x therefore moves the price twice as much at one end as the',
      'other. A percent holds that constant — which is how the wallets worth copying',
      'size, and why their realised slippage is flat (1.2-1.9% of vsol).',
      '',
      'Capped at 10%. If the pool depth is unknown at entry the rule falls back to the',
      'fixed amount rather than guessing a size.',
      '',
      'The grouped sweep IGNORES this and prices every combo at its flat notional —',
      're-run a percent-sized rule through simulate before trusting a sweep PnL.',
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
      'Create/promote auto-fills a compact label from the axes (`3ix:Buy · max=1 · bkt=1`).',
      'Edit it to a nickname any time — identity stays on the axes, not the name.',
      '',
      'Many rules can share one fingerprint.',
    ].join('\n'),
  },
  wildcard: {
    title: 'Match every token (wildcard)',
    body: [
      'This fingerprint matches EVERY token, ignoring every axis below.',
      '',
      'For a rule that decides purely on the tape (flow / price / burst metrics) and has',
      'no launch shape to name. A rule always needs a fingerprint, and clearing every axis',
      'means match NOTHING — the matcher refuses a criterion-less row on purpose, so a',
      'half-filled form can never arm on everything. Hence this switch: "any token" is said',
      'out loud, never inferred from a blank form.',
      '',
      'Mutually exclusive with the axes — turning it on drops them (an axis alongside a',
      'wildcard would read as a filter that never applies).',
      '',
      'Affects the LIVE entry gate: every rule on this fingerprint arms on every launch,',
      'so the entry conditions are the only thing left selecting what you trade.',
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
  ix_patterns: {
    title: 'Volume-side ix patterns',
    body: [
      'Ordered instruction-label sequences that TAG a trade for',
      'm_flow_ix / m_flow_ix_window. Exact ordered match — same vocabulary as fingerprint ix_labels.',
      '',
      'Classifier (any one is enough):',
      '  1) trade ix_labels hash ∈ one of these pattern rows',
      '  2) wallet already tagged on THIS token (contagion)',
      '  3) creator wallet (always tagged)',
      'Otherwise the trade is untagged (untagged_*).',
      '',
      'Example row: ["Pump.Fun: Buy","Token Program: CloseAccount"]',
      '',
      'Empty / missing m_flow_ix key ⇒ m_flow_ix / m_flow_ix_window metrics',
      'are NaN (never fire). Aggregate flow (m_flow_lifetime / m_flow_window) does not',
      'use this config.',
      'Discover candidates on Lab → Flow discovery, then Apply back here.',
    ].join('\n'),
  },
} as const satisfies Record<string, HelpTip>;

// ── Execution models (fill + cost) ───────────────────────────────────────────

/** The Fill / Cost dropdowns, shared by Simulate, dry-run and the sweep config —
 *  one help SSOT so all three read identically. Worked numbers and the full
 *  pairing matrix: docs/plans/strategies/fill-and-cost-models.md. */
export const EXECUTION_MODEL_HELP = {
  fillModel: {
    title: 'Fill model — which print you transact at',
    body: [
      'A signal never fills at the trade that triggered it. Candidates are the trades AFTER it',
      'in a short window: the signal’s own slot, plus the next observed slot if it lands within',
      '3 slots (~1s). This picks WHICH candidate prices the leg.',
      '',
      'ENTRY EXAMPLE — trigger prints 1.0, then buys 1.2 (same slot), 1.5 and 1.8 (next slot):',
      '  Worst-case        → 1.8   the highest buy in the window',
      '  First-in-window   → 1.2   the first buy after the trigger',
      '  Signal price      → 1.0   the trigger’s own spot',
      '',
      'EXIT EXAMPLE — fire prints 1.0, then 1.4 (same slot), sell 1.1 and buy 1.3 (next slot):',
      '  Worst-case        → 1.1   the lowest print in the window',
      '  First-in-window   → 1.4   the first print after the fire — here BETTER than both others',
      '  Signal price      → 1.0   the fire’s own spot',
      '',
      'First-in-window is “whatever printed next”, not a midpoint. Worst-case mirrors per side:',
      'highest buy on entry, lowest print on exit.',
      '',
      'THE TAKEN SET IS IDENTICAL across all three — same eligibility, only the price moves. So',
      'switching model reprices a fixed trade set; it never changes which trades you took.',
      '',
      'WHICH TO USE:',
      '  • Worst-case      the floor, and the only setting matching live paper (which cannot',
      '                    choose). Right for stops: a stop fires because price is falling, so',
      '                    the next prints really are lower. Penalises short holds hardest.',
      '  • First-in-window the realistic fast bot — use it to ask “is there edge at all”.',
      '  • Signal price    zero slippage, an upper bound. If a rule loses HERE, no amount of',
      '                    speed can save it.',
      '',
      'Run all three and read the spread: +8 / +2 / −4 is a latency bet, +6 / +5 / +4 is edge.',
      'Two runs under different fill models are not comparable — that is why it is stored on',
      'the run and shown on its header.',
    ].join('\n'),
  },
  costModel: {
    title: 'Cost model — frictions charged on top of the fill',
    body: [
      'Charged per leg (a round trip = 1 entry + N exit legs):',
      '  • pump.fun fee   125 bps of the leg (measured, not assumed)',
      '  • tip + priority 0.000225 ◎ fixed, from JITO_MIN_TIP_SOL + the CU priority fee',
      '  • price impact   our own buy_amount / reserve_sol  (Fee + real impact only)',
      '',
      'EXAMPLE — 0.1 ◎ buy, price rises 20%, 70 ◎ pool (the measured median depth):',
      '  Fee + real impact  +16.46%   charges 0.143%/leg of impact',
      '  Fee only           +16.80%   no impact at all',
      '  Fee + slippage     +14.45%   flat 1%/leg',
      '',
      'THE SAME TRADE AT 1.0 ◎ into the same pool:',
      '  Fee + real impact  +13.87%   impact is now 1.43%/leg',
      '  Fee only           +17.21%',
      '  Fee + slippage     +14.86%',
      '',
      'Read those together: the legacy flat 1% is too HARSH at 0.1 ◎ and too KIND at 1.0 ◎ —',
      'wrong in both directions, and it flips somewhere in between, so it does not even',
      'preserve ranking between combos of different size.',
      '',
      'WHICH TO USE:',
      '  • Fee + real impact  the default choice — the only size-aware model.',
      '  • Fee only           a clean upper bound (0.3pp generous at 0.1 ◎, 3.3pp at 1.0 ◎).',
      '  • Fee + slippage     legacy. It DOUBLE-COUNTS: the fill model already prices which',
      '                       print you hit, and slippage_bps is a flat stand-in for the same',
      '                       thing. It stays the wire default only so stored runs keep their',
      '                       meaning — never pick it for a new run.',
      '',
      'Impact is NOT double-counting: it is our own footprint on the curve, orthogonal to which',
      'print we hit, and a live trade pays both. Without pool depth no impact is charged, so',
      '“Fee + real impact” silently degrades to “Fee only”.',
      '',
      'BREAK-EVEN (fee + impact, 70 ◎ pool): +3.28% gross at 0.1 ◎, +3.26% at the 0.1255 ◎',
      'optimum, +5.55% at 1.0 ◎. Cost is U-shaped in size — the tip is fixed per leg, impact',
      'grows with size. Check a candidate against that bar before running a backtest.',
    ].join('\n'),
  },
} as const satisfies Record<string, HelpTip>;

// ── Sweep config / axes ──────────────────────────────────────────────────────

export const SWEEP_FIELD_HELP = {
  seedFingerprint: {
    title: 'Scope by saved fingerprint',
    body: [
      'When set, the sweep corpus keeps only tokens that MATCH this fingerprint',
      '(engine match SSOT — exact axes exact, continuous SOL axes by bucket, same',
      'gate the live entry arms on).',
      '',
      'The manual value filters below are then IGNORED — they compare exact values,',
      'so they cannot express a bucket axis. Group-by still applies: leave it empty',
      'for one “ALL” group over the matched tokens, or check fields to partition',
      'inside that slice.',
      '',
      'Leave empty to select the corpus with the manual group-by / filters instead.',
    ].join('\n'),
  },
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
      'to the scalar scan and says so. Combos whose exit conditions the scan can’t classify',
      '(token-scoped metrics, “=” bands, multi-arm OR) use the scalar walk either way.',
      '',
      'Leave it OFF. The default path is an O(log n) prefix-extrema index that already',
      'collapsed this scan: measured head-to-head on 4541 tokens x 1600 TP/SL combos, off',
      'is 3.2 s and on is 4.3 s. The toggle survives for A/B, not for speed.',
    ].join('\n'),
  },
  fillModel: EXECUTION_MODEL_HELP.fillModel,
  costModel: EXECUTION_MODEL_HELP.costModel,
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
    title: 'Window size',
    body: [
      'Trailing window for dynamic metrics on this axis (same as the size param on',
      'a rule): m_flow_window, m_flow_ix_window, m_price_window.',
      '',
      'Counted in the unit beside it — seconds, slots or prints. The axis assembles',
      'the size param that unit spells, so a slot axis sweeps window_size_slots and',
      'a print axis sweeps window_size_prints.',
      '',
      'You can sweep the same group at several windows (each becomes its own',
      'clause). Lifetime / static metrics (m_flow_lifetime, m_flow_ix) ignore this',
    ].join('\n'),
  },
  axisWindowUnit: {
    title: 'Window unit',
    body: [
      'What this axis’s windows count in.',
      '',
      '  • sec — wall clock, a continuous interval.',
      '  • slot — exactly N slots, the discrete buckets the chain batches in. A',
      '    bundle is a slot fact; at ~400ms a one-second window straddles two or',
      '    three slots and merges bursts that landed separately.',
      '  • print — exactly N transactions of this token’s own tape. Silence never',
      '    decays it, and size 1 lag 0 is ONE trade — the only span in which a',
      '    threshold is a statement about a trade rather than about an interval.',
      '',
      'Both the size and the lag count in this unit. Sweeping one metric on two units',
      'is two axes: they read different tape, so they are different rules.',
    ].join('\n'),
  },
  axisWindowLag: {
    title: 'Window lag',
    body: [
      'How many units back from now this axis’s window ENDS. 0 — the default — means',
      'it ends at now.',
      '',
      'This is what makes a swept window causal in its own terms. A gate on “the state',
      'entering this slot” must not be able to see the slot it fires in: the slice is',
      'size 1 / lag 0 and the quiet tape before it is size 30 / lag 1, two spans with',
      'no way for one to leak into the other.',
      '',
      'A lagged window reads a different span from an unlagged one of the same size,',
      'so the two are different clauses and never merge into one group instance.',
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
      'UI then uses one “ALL” group; Apply writes ix_patterns back to this fingerprint.',
      'Leave empty to partition manually with group-by / filters below.',
    ].join('\n'),
  },
  applyFingerprint: {
    title: 'Apply to fingerprint',
    body: [
      'Target fingerprint that receives the draft ix_patterns on Apply.',
      '',
      '• Pick an existing row → PUT metric_config.m_flow_ix on that fingerprint.',
      '• Empty → create / bind a fingerprint from the selected group key, then write patterns.',
      '',
      'Auto-match highlights a saved fingerprint whose axes already equal this group.',
    ].join('\n'),
  },
  draftPatterns: {
    title: 'Draft ix_patterns',
    body: [
      'Checked structures from the table become pattern rows (exact ix_labels sequences).',
      'Edit freely, then Apply to write m_flow_ix.ix_patterns on the target fingerprint.',
      '',
      'Those patterns drive tagged_* / untagged_* / tagged_share on rules that use this fingerprint.',
      'Empty draft cannot Apply — toggle at least one structure (or add a row in the editor).',
    ].join('\n'),
  },
  volumeSplit: {
    title: 'Flow split — checked structures',
    body: [
      'Live preview of the group’s scored SOL split into two buckets based on the checkboxes',
      'below: "Volume" = every row you’ve checked (would-be ix_patterns); "Organic" =',
      'every unchecked row. Nothing here is saved — it’s just a preview of what Apply would',
      'flag as volume once you toggle rows, computed client-side from each row’s Gross◎.',
      '',
      'Example: group scored 100 SOL total; you check two rows worth 62 SOL combined →',
      'bar shows 62% volume / 38% organic. Only structures currently listed in the table',
      '(top 64 by rank) count toward the total.',
    ].join('\n'),
  },
} as const satisfies Record<string, HelpTip>;

// ── Creation-stats dashboard ("Creation by token group") ─────────────────────

export const CREATION_FIELD_HELP = {
  seedFingerprint: {
    title: 'Scope by saved fingerprint',
    body: [
      'When set, the dashboard keeps only tokens that MATCH this fingerprint',
      '(engine match SSOT — exact axes exact, continuous SOL axes by bucket, same',
      'gate the live entry arms on) and shows them as a single "ALL" group.',
      '',
      'The manual group-by / value filters below are then IGNORED — they compare',
      'exact values, so they cannot express a bucket axis.',
      '',
      'Leave empty to partition the corpus with the manual group-by / filters instead.',
    ].join('\n'),
  },
} as const satisfies Record<string, HelpTip>;

/** Column tips for the discovery structure ranking table. */
export const DISCOVERY_COL_HELP = {
  vol: {
    title: 'Vol — include in draft',
    body: [
      'Check this box to add the row’s exact ix_labels sequence to the draft',
      'ix_patterns list below the table. Unchecked rows are ignored on Apply —',
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
  overlap: {
    title: 'Overlap 0–1 — the same wallets, token after token?',
    body: [
      'Average overlap between the wallet sets that traded this shape on any two tokens of',
      'the group (Jaccard: shared wallets ÷ all wallets across the pair).',
      '',
      'Example: token A’s shape was traded by {w1,w2,w3} and token B’s by {w1,w2,w4} →',
      '2 shared ÷ 4 total = 0.5.',
      '',
      'High → ONE crew is running this shape across the group’s launches — the strongest',
      'evidence on this table that it’s tooling, not traders. 0 → every token drew a fresh',
      'set of wallets (or fewer than two tokens carry the shape, so there’s nothing to',
      'compare).',
      '',
      'Reuse and Overlap answer different questions: Reuse is "few wallets did many trades',
      'HERE", Overlap is "the SAME wallets came back on the next token". A farm that fires',
      'one trade per wallet scores 0 Reuse but high Overlap.',
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
  firstSlot: {
    title: 'Launch% — how much of this shape happened at launch?',
    body: [
      'This shape’s SOL that landed in the token’s CREATION slot ÷ this shape’s total SOL',
      '× 100 — i.e. how purely this is launch tooling (dev buy / bundle) rather than a shape',
      'that also trades later.',
      '',
      'Example: a ["Pump.Fun: Create","Pump.Fun: Buy"] row whose every trade lands in the',
      'creation slot → Launch% = 100. A plain ["Pump.Fun: Buy"] row bought once by a bundler',
      'at launch and 99 more times by real buyers minutes later → Launch% ≈ 1.',
      '',
      '100 → this shape ONLY ever appears at launch: bundler/sniper tooling. Mid-range →',
      'mixed: the same shape carries launch AND organic flow, and live classifies by ix_labels',
      'alone (no slot test), so checking it also tags the organic tail — and, via wallet',
      'contagion, those wallets’ other trades. "—" = a result cached before this column',
      'existed; re-run discovery to fill it.',
      '',
      'The % is informational — "Launch shapes · group" takes every row with at least ONE',
      'creation-slot trade, whatever its Launch% and whatever its size, because the launch',
      'bundle is the set of shapes that appear in that slot. Badged rows are exactly the ones',
      'that button will check; read the % to see how much organic tail each drags in.',
      '',
      'That button is GROUP-wide and reads this table, so it answers "appeared in SOME matched',
      'token\'s creation slot" and can only offer rows that survived the server-side row cap.',
      'For one token\'s actual bundle, pick it in the preview panel and use "Launch shapes ·',
      'this token" — per-token, uncapped, and not read off this table.',
      '',
      'The creation slot is taken as the slot of the token’s first trade, so a token whose',
      'launch slot had no trade at all reports its first later slot instead.',
    ].join('\n'),
  },
  suggested: {
    title: 'Auto — is this shape auto-flagged as volume?',
    body: [
      'A client-side verdict that composites the bot-likelihood columns into one call, so',
      'you don’t have to eyeball every row. The % IS the decision: badge at ≥ 50%, dim',
      'text below it, "—" when a gate blocked the row. Hover any cell for the full',
      'breakdown — every family, its value, and why it did or didn’t count.',
      '',
      'Gates (any one blocks the row): Gross◎ ≥ 0.05 (no dust); the shape moved meaningful',
      'SOL on ≥ 2 of the group’s tokens (a pattern is written onto the whole FINGERPRINT,',
      'so a one-token curiosity is out of scope); and — only when the run HAS an',
      'out-of-group baseline — Lift × ≥ 1.25. A fingerprint-scoped run is one group over',
      'the whole corpus, so its lift is 1.00 by construction: the gate is skipped there,',
      'not failed.',
      '',
      'The score averages four FAMILIES of evidence, not four columns — correlated columns',
      'collapse into one so a single fact can’t vote twice (one launch bundle trips both',
      'same-slot bursts and few-wallets):',
      '  Recur   — the shape repeats across the group’s tokens',
      '  Burst   — its trades cluster within ±1 slot',
      '  Wallets — best of Reuse and Overlap (Reuse ignored under 4 trades, where 2 trades',
      '            from 1 wallet would read 0.5 off a coin flip)',
      '  Wash    — buys and sells cancel (both-sided rows only; n/a is left out, not zeroed)',
      '',
      'Averaging families is what encodes "needs about two kinds of evidence": one family',
      'at 100% with the rest cold lands near 25–33% and cannot pass alone.',
      '',
      'Contagion% is deliberately NOT an input. It is defined against what you have already',
      'checked, so folding it in made the same row score differently depending on your click',
      'order and made the bulk-select non-idempotent. It stays a column you read when',
      'deciding by hand.',
      '',
      'Suggestions are advisory — hover "Auto-select suggested" to outline exactly which rows',
      'it would check, and you can still toggle any row by hand.',
    ].join('\n'),
  },
} as const satisfies Record<string, HelpTip>;

// ── Rules board (view controls, not rule fields) ─────────────────────────────

/**
 * The "Score all rules on this ledger" modifier. Written as a builder rather than
 * a constant because the whole point of the control is that it re-reads the mode
 * picker beside it — a tip that says "this mode" while the picker says Real is the
 * same ambiguity the control exists to remove. `mode` is the picker's current
 * value; `null` = the picker is on All, where the box is disabled.
 */
export function scoreLedgerHelp(mode: 'paper' | 'real' | null): HelpTip {
  const m = mode ?? 'real';
  const other = m === 'real' ? 'paper' : 'real';
  // Column-aligned by padding, not by hand-typed spaces: `real` and `paper` are
  // different widths, so a literal figure would be square in one mode and ragged
  // in the other.
  const row = (runs: string, shows: string) => `  ${`${runs} rule`.padEnd(13)}▸  ${shows}`;
  return {
    title: 'Score all rules on this ledger',
    figure: [
      `picker = ${m.toUpperCase()}${mode === null ? '   (example)' : ''}`,
      '',
      'OFF · picker = a ROW FILTER',
      row(m, `${m} ◎`),
      row(other, 'row hidden'),
      '',
      'ON  · picker = a LEDGER',
      row(m, `${m} ◎`),
      row(other, `${m} ◎  [${m}]`),
      `                  ^ on screen,`,
      `                    runs ${other}`,
    ].join('\n'),
    body: [
      `This box changes what the Paper/Real picker beside it MEANS. It never changes what any rule does — no rule starts or stops trading, and nothing is written.`,
      '',
      `OFF — the picker is a ROW FILTER (label reads "Show").`,
      `  Picking ${m} hides the ${other} rules. Each remaining rule is scored on its OWN trade mode, so a paper rule shows paper numbers and a real rule shows real ones. This is the keep/kill board, and the default.`,
      '',
      `ON — the picker names a LEDGER (label reads "Score on").`,
      `  Nothing is hidden: ${other} rules stay on screen, but every rule is now scored on its ${m} positions. This is the only way to read a rule's ${m} record while it runs ${other}, and the only basis on which rules in different modes rank against each other — paper and real PnL are different currencies (one is money, one is not), so an "own mode" board never compares them honestly.`,
      '',
      'WHAT CHANGES ON SCREEN',
      `  · The scoreboard tiles, every score column, and the TOTAL strip re-query on the ${m} ledger — these are real per-mode figures from the server, not a client-side filter of what was already loaded.`,
      `  · A rule with no ${m} positions reads "—", not 0. It has no record here, which is not the same as a flat one.`,
      `  · Rules that run ${other} get a small ${m} pill beside their PnL: the figure is genuine history, but not that rule's live ledger, and nothing else in the row would say so. The strip below counts them ("N of M rules are not running ${m}").`,
      `  · The TOTAL strip files each figure under the mode the NUMBERS came from, never the mode the rule is switched to — otherwise a pinned ledger would quietly blend simulated SOL into a real total.`,
      '',
      'INDEPENDENT OF Span',
      '  Span (Current run / All-time) picks WHICH positions count; this box picks WHOSE ledger they come from. Both apply at once.',
      '',
      mode === null
        ? 'Disabled right now because the picker is on All — "All" names no ledger to score on. Pick Paper or Real first.'
        : 'Needs Paper or Real: on All the box is disabled, since "All" names no ledger to score on.',
    ].join('\n'),
  };
}
