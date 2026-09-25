/** Plain-language ⓘ help for the PAGE-level fields of rule / fingerprint / sweep
 *  authoring (name, mode, caps, cost models, ...).
 *
 *  Every metric, span, tag matcher and rule part is explained by the registry
 *  (`GET /api/meta/strategy-registry`), never here: one definition, rendered as-is.
 *  Bodies may use newlines (`whitespace-pre-line` in InfoTooltip). */

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
    'Conditions in one list must ALL hold. For "this OR that" across metrics, write two',
    'lines, or a signal with two groups.',
  ].join('\n'),
};

// ── Rule page fields ─────────────────────────────────────────────────────────

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
    title: 'Labels',
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
      'Locked after create — unlock the padlock to flip paper↔real. That only affects future buys; open positions keep their original mode. Sizing/caps stay editable while live; the fingerprint and the conditions lock once active.',
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
      'Blank (∞) = unlimited. After N entries, the rule stops taking new tokens.',
      'A run lasts until the rule is switched off or its trade mode changes: a restart keeps the count, switching the rule back on starts a new run and a new count.',
    ].join('\n'),
  },
  fingerprint: {
    title: 'Fingerprint',
    body: [
      'Creation-time matcher: which brand-new tokens this rule is allowed to arm on (CU, first buy, labels, etc.).',
      '',
      'Flow: fingerprint matches → the rule watches the coin → Buy conditions → buy → Sell lines.',
      '',
      'Its tags (e.g. @volume) are the trade lists the rule\'s conditions can read.',
      '',
      'Locked while the rule is live so live behaviour cannot silently change mid-run.',
    ].join('\n'),
  },
  paramsJson: {
    title: 'Params JSON',
    body: [
      'The rule as stored (format 2): enter{event, filters, final_filters, lock,',
      'size_pct_of_pool}, take_profit, stop_loss, signals{}, always[], stages[], reentry,',
      'exclusive, priority.',
      '',
      'A condition: {"metric": "m_flow.buy_sol", "tag": "!volume", "span": "10s",',
      '"is": [{"operator": ">=", "value": 2}]}. A line: {"if": [conditions],',
      '"sell": "label", "go": "stage"}.',
      '',
      'Checked against the registry on save. The Builder tab writes exactly this.',
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
      '',
      'THE SAME TRADE AT 1.0 ◎ into the same pool:',
      '  Fee + real impact  +13.87%   impact is now 1.43%/leg',
      '  Fee only           +17.21%',
      '',
      'WHICH TO USE:',
      '  • Fee + real impact  the default choice — the only size-aware model.',
      '  • Fee only           a clean upper bound (0.3pp generous at 0.1 ◎, 3.3pp at 1.0 ◎).',
      '',
      'A third model once charged a flat 1%/leg. It is gone: at 0.1 ◎ it read +14.45% (too',
      'HARSH against the real +16.46%) and at 1.0 ◎ +14.86% (too KIND against +13.87%) — wrong',
      'in both directions, flipping somewhere in between, so it did not even preserve ranking',
      'between combos of different size. It also double-counted, since the fill model already',
      'prices which print you hit. A run still labelled with it has been REPRICED, not',
      'reproduced: distrust the numbers stored beside it.',
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
      '',
      'Picking one also loads its tags into the run\'s tags, so an axis can read them.',
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
  axes: {
    title: 'Sweep axes',
    body: [
      'Each axis is one dimension of the grid; a combo picks one value from every axis',
      'and runs as one rule.',
      '',
      '  entry axis  a condition the coin must pass to be bought (the rule\'s "Only if").',
      '  exit axis   its own sell line: sells everything when the condition holds.',
      '  TP / SL     the rule\'s take-profit / stop-loss %.',
      '',
      'Example: entry m_state.age_sec <= 20, 60 and TP 50, 100 is 2 x 2 = 4 combos.',
      '',
      'Two axes on the same read (metric + tag + span) join into one condition: AND when',
      'both can hold (> 5 and < 50), else OR (< 5 or > 50). Drag an axis onto the other',
      'column to flip its side.',
    ].join('\n'),
  },
  axisOp: {
    title: 'Operator',
    body: [
      'The comparison every value on this axis is tried with: >  >=  <  <=  =  !=',
      '',
      'Example: >= with values 1, 2 is two combos, one reading >= 1 and one >= 2.',
    ].join('\n'),
  },
  tags: {
    title: 'Run tags',
    body: [
      'The named trade lists this run\'s @tag reads use, the same document a fingerprint',
      'carries. Picking a scope fingerprint loads its tags; edit them here for the run.',
      '',
      'Sent only when an axis reads a tag. Promote writes them onto the promoted',
      'fingerprint, so the saved rule reads the trades the sweep scored with.',
      '',
      'Example: tag volume = trades by program Axiom; an axis m_flow.buy_sol @!volume [10s]',
      'then reads SOL bought by everyone else in the last 10 s.',
    ].join('\n'),
  },
  stagePlans: {
    title: 'Stage plan (Pass 2)',
    body: [
      'One plan of stages to try on top of the grid. After ranking, each group\'s',
      'top-K combos are re-scored under this plan AND under their own exit; each combo',
      'keeps whichever scores better, and Promote saves that rule.',
      '',
      'A plan reads our position only (m_position metrics, TP/SL): the sweep records no',
      'other columns for it.',
      '',
      'Example: stage bank sells 70 % at m_position.pnl_pct >= 50 and goes to rest;',
      'rest sells the remainder at m_position.held_sec >= 30.',
      '',
      'Author a plan you already believe in: trying many shapes on the same small',
      'per-combo sample picks noise.',
    ].join('\n'),
  },
  stagePlansTopK: {
    title: 'Top-K per group (Pass 2)',
    body: [
      'How many of each group\'s best combos the stage plan re-scores.',
      '',
      'Example: 3 re-scores the three best combos of every group; the rest keep their exit.',
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
      'UI then uses one “ALL” group; Apply writes the checked shapes into one of this',
      'fingerprint’s tags. Leave empty to partition manually with group-by / filters below.',
    ].join('\n'),
  },
  applyFingerprint: {
    title: 'Apply to fingerprint',
    body: [
      'Target fingerprint whose tag receives the draft on Apply.',
      '',
      '• Pick an existing row → the draft is written into the chosen tag on that fingerprint;',
      '  its other tags and matchers are kept.',
      '• Empty → create / bind a fingerprint from the selected group key, then write the tag.',
      '',
      'Auto-match highlights a saved fingerprint whose axes already equal this group.',
    ].join('\n'),
  },
  draftPatterns: {
    title: 'Draft for a tag',
    body: [
      'Exact shapes: checked structures become the tag’s ix_shape entries (exact ix_labels, any fee budget). Checking Vol on a shape that already has pins widens those pins to the catch-all.',
      'On the preview trades table, the pin checkboxes (cu_limit / cu_price / tip) copy those fields from the clicked tx. A pin click on a catch-all of that shape narrows it to that pin — the two never sit together, because the catch-all already matches every budget.',
      'An ix-only row is a wildcard: Vol stays checked on every trade of that shape. A pin-only row lights only the trades that match that pin.',
      'Templates: a grain (program|CU|ATA|N|S|F) goes under ix_template; a bare program name goes under program and matches every grain of that router. Launch (create) shapes are skipped. No fee pins and no wallet contagion. Templates need a saved fingerprint — pick one to Update.',
      'Apply writes the draft into the chosen tag on the target fingerprint; every other tag and matcher is kept.',
      '',
      'Rules on this fingerprint then read the split: m_flow.buy_sol @volume is the SOL bought by trades carrying the tag, m_flow.buy_sol @!volume by the rest.',
      'An empty draft cannot create a fingerprint — toggle at least one structure (or add a row in the editor).',
    ].join('\n'),
  },
  volumeSplit: {
    title: 'Flow split — checked structures',
    body: [
      'Live preview of the group’s scored SOL split into two buckets based on the checkboxes',
      'below: "Volume" = every row you’ve checked (the would-be @tag trades); "Organic" =',
      'every unchecked row (@!tag). Nothing here is saved — it’s just a preview of what Apply',
      'would tag once you toggle rows, computed client-side from each row’s Gross◎.',
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
      'Check this box to add the row’s exact ix_labels sequence to the tag’s draft',
      'ix_shape list as a catch-all (any fee budget). If that shape already has',
      'fee pins, checking Vol widens them to the catch-all. Unchecked rows are ignored',
      'on Apply unless a fee-pinned copy of the same shape is already in the draft.',
      'Pin cu_limit / cu_price / tip from a specific tx on the preview trades strip —',
      'that click narrows a catch-all of this shape rather than adding a second row.',
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
  budget: {
    title: 'Budget — the fee this build was sent with',
    body: [
      'The compute budget the sender declared, as distinct (cu_limit, cu_price, tip) triples.',
      'The cell shows the most-traded one and, when there is more than one, how many exist.',
      '',
      'THIS is what decides whether a budget is identity or noise. One triple covering nearly',
      'every trade is a preset compiled into the operator’s client — pin it and the entry gets',
      'sharper. A long tail means the client reads a fee oracle per transaction: pinning any',
      'one value there matches the single transaction you copied it from and then silently',
      'never fires again.',
      '',
      'cu_limit is usually a preset and pins well. A tip is an auction bid and almost never does.',
      'cu_price is the one to check the count on before trusting.',
      '',
      '"unknown" counts trades carrying no fee reading at all — everything older than the',
      'forward-only fee capture. A build that is all-unknown cannot be pinned yet, however',
      'stable it looks.',
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
      'you’ve already checked above. Shown for a STICKY tag, where that is how the engine',
      'classifies: once a wallet’s trade matches a checked shape, ALL of that wallet’s later',
      'trades (any side, any shape) carry the tag too, not just the matching row.',
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
