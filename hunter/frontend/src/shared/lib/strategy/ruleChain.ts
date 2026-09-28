// The short chain a rule shows in the editor and on the Rules list.
// A signal is its own name, above buy and sell: one definition, named from
// any gate. Watch, buy, size, and again are one line each. Sell is one name
// per step (TP/SL, always, then each stage). TP/SL is one chip for the two
// inputs. A new stage is one more name. The lines of a step stay behind that name.

import { formatDecimalTrim } from 'utils/format';
import type { Cond, RuleDoc } from './ruleDoc';
import { condLabel } from './sentences';

export interface SellChip {
  key: string;
  label: string;
}

/** Buy gates. `when` is the buy. `looking` is how long the rule keeps trying. */
export type BuyKey = 'on' | 'if' | 'giveup' | 'lock';
export type BuyGroup = 'when' | 'looking';

export interface BuyChip {
  key: BuyKey;
  title: string;
  group: BuyGroup;
}

/** Miss lines for the two buy groups. The once-per-coin line is the one extra fact. */
export const BUY_WHEN_MISS = 'Miss: this try fails.';
export const BUY_WHEN_LAST = 'Once Per Coin has no next try, so this stops the coin.';
export const KEEP_LOOKING_MISS = 'Miss: stop this coin.';

function liveText(c: Cond): string | null {
  if (c.off) return null;
  if (c.kind === 'signal') return c.not ? `not ${c.signal}` : c.signal;
  const text = condLabel(c).trim();
  return text || null;
}

/**
 * The three Tries choices. Each hint is only the budget. A miss of Only if or
 * Give up is stated on that gate. Same facts as `enter.lock` in the registry.
 */
export const LOCK_CHOICES: { value: '' | 'token' | 'slot'; label: string; hint: string }[] = [
  {
    value: '',
    label: 'Any Print',
    hint: 'A try on every trade and every clock tick where On is true.',
  },
  {
    value: 'token',
    label: 'Once Per Coin',
    hint: 'One try: the first trade where On is true. A clock tick is never a try. No next try, so a failed Only If stops this coin.',
  },
  {
    value: 'slot',
    label: 'Once Per Slot',
    hint: 'One try per block, the first moment On is true.',
  },
];

export function lockWord(lock: RuleDoc['enter']['lock']): string {
  return LOCK_CHOICES.find((c) => c.value === (lock ?? ''))?.label ?? 'Any Print';
}

function gateShown(cs: Cond[], forEditor: boolean, editing: boolean): boolean {
  if (forEditor) return cs.length > 0 || editing;
  return cs.some((c) => !c.off);
}

/**
 * Buy when, then Keep looking. On and Only if are the buy. Tries is always
 * there. Give up is the abort. `forEditor` also keeps a gate that is open but
 * still empty, and a gate whose conditions are all off, so a parked condition
 * stays reachable.
 */
export function buyChips(doc: RuleDoc, forEditor = false, editing: BuyKey | null = null): BuyChip[] {
  const out: BuyChip[] = [];
  if (gateShown(doc.enter.event, forEditor, editing === 'on')) out.push({ key: 'on', title: 'On', group: 'when' });
  if (gateShown(doc.enter.filters, forEditor, editing === 'if')) out.push({ key: 'if', title: 'Only If', group: 'when' });
  out.push({ key: 'lock', title: lockWord(doc.enter.lock), group: 'looking' });
  if (gateShown(doc.enter.final_filters, forEditor, editing === 'giveup')) out.push({ key: 'giveup', title: 'Give Up', group: 'looking' });
  return out;
}

export function buyGateConds(doc: RuleDoc, key: BuyKey): Cond[] {
  if (key === 'on') return doc.enter.event;
  if (key === 'if') return doc.enter.filters;
  if (key === 'giveup') return doc.enter.final_filters;
  return [];
}

/** Buy line: the first On condition, a count when there are more, then the lock. */
export function buyGlance(doc: RuleDoc): string {
  const ons = doc.enter.event.map(liveText).filter((t): t is string => t != null);
  const lock = lockWord(doc.enter.lock);
  if (ons.length === 0) return `first print where the filters hold · ${lock}`;
  const more = ons.length > 1 ? ` +${ons.length - 1}` : '';
  return `${ons[0]}${more} · ${lock}`;
}

function tpslLabel(doc: RuleDoc): string {
  const parts: string[] = [];
  if (doc.take_profit != null) parts.push(`TP ${formatDecimalTrim(doc.take_profit, 1)}`);
  if (doc.stop_loss != null) parts.push(`SL ${formatDecimalTrim(doc.stop_loss, 1)}`);
  return parts.join(' · ') || 'TP/SL';
}

/**
 * The signal chip, above buy and sell. One name, or a count. `forEditor` keeps
 * the chip while the section is open and still empty. Absent when the rule
 * defines none.
 */
export function signalChip(doc: RuleDoc, forEditor = false, editing = false): string | null {
  if (doc.signals.length === 0 && !(forEditor && editing)) return null;
  if (doc.signals.length === 1) return doc.signals[0].name;
  if (doc.signals.length > 1) return `${doc.signals.length} Signals`;
  return 'Signal';
}

/**
 * Sell names on the chip line. TP/SL stays ahead of Always. `forEditor` also
 * keeps a step that is open but still empty, and an Always list whose lines
 * are all off, so a parked line stays reachable. The list omits those: it is
 * the live chain.
 */
export function sellChips(doc: RuleDoc, forEditor = false, editing: string | null = null): SellChip[] {
  const out: SellChip[] = [];
  if (doc.stop_loss != null || doc.take_profit != null || (forEditor && editing === 'tpsl')) {
    out.push({ key: 'tpsl', label: tpslLabel(doc) });
  }
  const alwaysLive = doc.always.some((l) => !l.off);
  if (alwaysLive || (forEditor && (doc.always.length > 0 || editing === 'always'))) {
    out.push({ key: 'always', label: 'Always' });
  }
  for (const s of doc.stages) out.push({ key: `stage:${s.id}`, label: s.name });
  return out;
}

export function sellGlance(doc: RuleDoc): string {
  const chips = sellChips(doc);
  return chips.length ? chips.map((c) => c.label).join(' → ') : 'no sell';
}
