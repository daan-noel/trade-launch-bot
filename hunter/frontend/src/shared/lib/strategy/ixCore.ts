// Build core: the TS mirror of `hunter_engine::metrics::trade_keys::{core_labels,
// core_marks}`. A core-level `ix_shape` row matches every variant of a build that
// keeps its core, so the chart must reduce a trade's labels exactly as the fold does.
// Both sides assert `hunter/engine/fixtures/ix_core_parity.json`.

import { isBoilerplate } from './templateGrain';

/** Engine `is_core_extra`: what a sender adds, drops or moves around a trade. */
export function isCoreExtra(label: string): boolean {
  return isBoilerplate(label) || label.startsWith('System Program:') || label.startsWith('Lighthouse');
}

/** Engine `core_verb`: pump.fun's verb variants merged; a merged verb maps to itself. */
function coreVerb(label: string): string {
  if (!label.startsWith('Pump.Fun: ')) return label;
  const rest = label.slice('Pump.Fun: '.length);
  if (rest.startsWith('Buy') || rest === 'BUY') return 'Pump.Fun: BUY';
  if (rest.startsWith('Sell') || rest === 'SELL') return 'Pump.Fun: SELL';
  if (rest.startsWith('Create') || rest === 'CREATE') return 'Pump.Fun: CREATE';
  return label;
}

/** The core: labels without the extras, in order, verbs merged. */
export function coreLabels(labels: readonly string[]): string[] {
  return labels.filter((l) => !isCoreExtra(l)).map(coreVerb);
}

/** Identity of a core, the key two builds compare by. */
export function coreKey(labels: readonly string[]): string {
  return JSON.stringify(coreLabels(labels));
}

/** Engine `CORE_MARK_FLAGS`, in bit and text order. */
export const CORE_MARK_FLAGS: readonly (readonly [string, (l: string) => boolean])[] = [
  ['CL', (l) => l === 'Compute Budget: SetComputeUnitLimit'],
  ['CP', (l) => l === 'Compute Budget: SetComputeUnitPrice'],
  ['N', (l) => l === 'System Program: AdvanceNonceAccount'],
  ['L', (l) => l.startsWith('Lighthouse')],
  ['M', (l) => l.startsWith('Memo Program')],
  ['S', (l) => l.startsWith('System Program: CreateAccount')],
  ['C', (l) => l.endsWith(': CloseAccount')],
  ['W', (l) => l.endsWith(': SyncNative')],
];

/** Engine `core_marks`: presence flags in bits 0-7, the System transfer count in
 *  bits 8-15 and the account-open count in bits 16-23 (each capped at 255). */
export function coreMarks(labels: readonly string[]): number {
  let flags = 0;
  let transfers = 0;
  let opens = 0;
  for (const l of labels) {
    CORE_MARK_FLAGS.forEach(([, test], bit) => {
      if (test(l)) flags |= 1 << bit;
    });
    if (l === 'System Program: Transfer') transfers += 1;
    if (l.startsWith('Associated Token: Create')) opens += 1;
  }
  return flags | (Math.min(transfers, 255) << 8) | (Math.min(opens, 255) << 16);
}

/** Engine `core_marks_text`: `"CL CP C T1 A3"`. */
export function coreMarksText(marks: number): string {
  const out = CORE_MARK_FLAGS.filter((_, bit) => (marks & (1 << bit)) !== 0).map(([n]) => n);
  out.push(`T${(marks >> 8) & 0xff}`, `A${(marks >> 16) & 0xff}`);
  return out.join(' ');
}

/** Engine `core_marks_from_text`; `null` on a token the engine refuses. */
export function coreMarksFromText(text: string): number | null {
  let marks = 0;
  for (const tok of text.split(/\s+/).filter(Boolean)) {
    const bit = CORE_MARK_FLAGS.findIndex(([n]) => n === tok);
    if (bit >= 0) {
      marks |= 1 << bit;
      continue;
    }
    const m = /^([TA])(\d+)$/.exec(tok);
    if (!m) return null;
    const n = Number(m[2]);
    if (n > 255) return null;
    const shift = m[1] === 'T' ? 8 : 16;
    marks = (marks & ~(0xff << shift)) | (n << shift);
  }
  return marks;
}
