// Build-template grain — the TS mirror of `hunter_engine::metrics::template_grain::grain`.
// `program|CU|ATA|N|S|F`. Not a full `ix_labels` sequence: harvest's working list
// is this grain, and a tape click that adds a trade to that list must spell the
// same id the fold hashes.

const BOILERPLATE_EXACT = new Set([
  'System Program: Transfer',
  'System Program: AdvanceNonceAccount',
  'System Program: CreateAccountWithSeed',
  'System Program: CreateAccount',
]);

function isBoilerplate(label: string): boolean {
  return (
    label.startsWith('Compute Budget:') ||
    label.startsWith('Associated Token:') ||
    label.startsWith('Token Program:') ||
    label.startsWith('Token 2022:') ||
    label.startsWith('Memo Program:') ||
    BOILERPLATE_EXACT.has(label)
  );
}

/** Program name — SQL `ixg.program`. */
export function templateProgram(labels: readonly string[]): string {
  const head = labels.find((x) => !isBoilerplate(x)) ?? '(direct)';
  if (head === '(direct)') return '(direct)';
  if (head.startsWith('Pump.Fun: Create')) return 'launch';
  if (head.startsWith('Pump.Fun:')) return 'Pump.Fun';
  return head.split(':')[0] ?? head;
}

/** Durable template id — SQL `tmpl`: `program || |CU || |ATA || |N || |S || |F`.
 *
 *  Empty input still returns `(direct)` (no flags). Callers that mean "missing
 *  labels" should not add this to a working list. */
export function templateGrain(labels: readonly string[]): string {
  let s = templateProgram(labels);
  if (labels.some((l) => l.startsWith('Compute Budget:'))) s += '|CU';
  if (labels.some((l) => l.startsWith('Associated Token:'))) s += '|ATA';
  if (labels.includes('System Program: AdvanceNonceAccount')) s += '|N';
  if (labels.includes('System Program: CreateAccountWithSeed')) s += '|S';
  if (labels.includes('System Program: Transfer')) s += '|F';
  return s;
}

/** True when any label is a pump.fun create. Those prints do not join the
 *  `m_burst_slot` member prefix. */
export function isLaunchGrain(labels: readonly string[]): boolean {
  return labels.some((l) => l.startsWith('Pump.Fun: Create'));
}

/** Split pasted grain-id text into trimmed unique ids, preserving first-seen order. */
export function parseGrainIds(text: string): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const raw of text.split(/[\n,]+/)) {
    const id = raw.trim();
    if (!id || seen.has(id)) continue;
    seen.add(id);
    out.push(id);
  }
  return out;
}

/** Remove `id` if the list already has it, else append. Membership, not position. */
export function toggleWorkingTemplate(list: readonly string[], id: string): string[] {
  const grain = id.trim();
  if (!grain) return list.map((x) => x);
  const kept = list.filter((x) => x !== grain);
  if (kept.length !== list.length) return kept;
  return [...list, grain];
}
