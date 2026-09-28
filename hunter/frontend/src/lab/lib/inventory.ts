/**
 * Parser for `hunter/docs/plans/strategies/_!___inventory.md`, the idea list.
 * The markdown file is the one source; the Inventory page renders what this
 * returns and holds no copy of any idea. The format it expects is the file's own
 * "What a row is" section: `## <slot> - <title>`, `### <family id> <title>`, and
 * seven-column idea tables. `inventory.test.ts` parses the real file, so a format
 * change fails the test instead of dropping rows from the page.
 */

export const STATUSES = ['keep', 'open', 'red', 'dead', 'new'] as const;
export type IdeaStatus = (typeof STATUSES)[number];

export interface Idea {
  name: string;
  slot: string;
  familyId: string;
  idea: string;
  meaning: string;
  why: string;
  /** The Example cell split on `<br>`: one entry per numbered step. */
  example: string[];
  parameters: string;
  status: IdeaStatus;
  /** The bracketed word after the status, e.g. `screen` in `keep (screen)`. */
  statusTag: string | null;
  /** The books that read the row: every `LIST_SEP` part after the status. */
  books: string[];
}

export interface Family {
  id: string;
  title: string;
  intro: string;
  ideas: Idea[];
}

export interface Slot {
  code: string;
  title: string;
  intro: string;
  families: Family[];
}

/** One cell of the structure grid: the row it names, the setting, and that setting's status. */
export interface GridCell {
  name: string;
  setting: string | null;
  status: IdeaStatus;
}

export interface StructureGrid {
  /** The grouping columns, after the first ("read") column. */
  columns: string[];
  rows: { label: string; cells: GridCell[] }[];
}

export interface Inventory {
  slots: Slot[];
  grid: StructureGrid;
}

export const IDEA_COLUMNS = [
  'Name',
  'Idea',
  'Meaning',
  'Why it matters',
  'Example',
  'Parameters',
  'Status',
] as const;

/** The file's list separator, a spaced middle dot. */
const LIST_SEP = ' \u00b7 ';

const SLOT_HEADING = /^## ([A-Z]) - (.+)$/;
const FAMILY_HEADING = /^### ([A-Z]\d+) (.+)$/;
const GRID_CELL = /^\*\*(.+?)\*\*(?:\s*\(([^)]*)\))?\s*\xb7\s*(keep|open|red|dead|new)$/;
const STATUS_HEAD = /^(keep|open|red|dead|new)(?:\s*\(([^)]*)\))?$/;

function cells(line: string): string[] {
  return line
    .trim()
    .replace(/^\|/, '')
    .replace(/\|$/, '')
    .split('|')
    .map((c) => c.trim());
}

const isRow = (line: string) => line.trimStart().startsWith('|');
const isSeparator = (line: string) => /^\|\s*-{3}/.test(line.trim());
const stripBold = (s: string) => s.replace(/^\*\*(.+)\*\*$/, '$1');

/** Split a document into `## ` sections, keeping each heading with its body. */
function sections(md: string): { heading: string; lines: string[] }[] {
  const out: { heading: string; lines: string[] }[] = [];
  for (const line of md.replace(/\r\n/g, '\n').split('\n')) {
    if (line.startsWith('## ')) out.push({ heading: line, lines: [] });
    else if (out.length > 0) out[out.length - 1].lines.push(line);
  }
  return out;
}

/** Rows of every table in `lines`, header and separator dropped. */
function tableRows(lines: string[]): string[][] {
  const rows: string[][] = [];
  let inTable = false;
  for (const line of lines) {
    if (!isRow(line)) {
      inTable = false;
      continue;
    }
    if (!inTable) {
      inTable = true; // header row
      continue;
    }
    if (!isSeparator(line)) rows.push(cells(line));
  }
  return rows;
}

/** Prose paragraphs before the first table or sub-heading, joined into one string. */
function intro(lines: string[]): string {
  const prose: string[] = [];
  for (const line of lines) {
    if (isRow(line) || line.startsWith('### ') || line.startsWith('```')) break;
    if (line.trim() === '---') continue;
    prose.push(line);
  }
  return prose
    .join('\n')
    .split(/\n\s*\n/)
    .map((p) => p.replace(/\s*\n\s*/g, ' ').trim())
    .filter(Boolean)
    .join('\n\n');
}

function parseIdea(row: string[], slot: string, familyId: string): Idea {
  if (row.length !== IDEA_COLUMNS.length) {
    throw new Error(
      `inventory ${familyId}: a row has ${row.length} cells, expected ${IDEA_COLUMNS.length}: ${row[0]}`,
    );
  }
  const [name, idea, meaning, why, example, parameters, statusCell] = row;
  const [head, ...books] = statusCell.split(LIST_SEP).map((s) => s.trim());
  const m = STATUS_HEAD.exec(head);
  if (!m) throw new Error(`inventory ${familyId} ${name}: unknown status "${head}"`);
  return {
    name: stripBold(name),
    slot,
    familyId,
    idea,
    meaning,
    why,
    example: example.split(/<br\s*\/?>/).map((s) => s.trim()).filter(Boolean),
    parameters: parameters === '-' ? '' : parameters,
    status: m[1] as IdeaStatus,
    statusTag: m[2] ?? null,
    books,
  };
}

function parseSlot(code: string, title: string, lines: string[]): Slot {
  const families: Family[] = [];
  let current: { id: string; title: string; lines: string[] } | null = null;
  const flush = () => {
    if (!current) return;
    families.push({
      id: current.id,
      title: current.title,
      intro: intro(current.lines),
      ideas: tableRows(current.lines).map((r) => parseIdea(r, code, current!.id)),
    });
  };
  const head: string[] = [];
  for (const line of lines) {
    const fm = FAMILY_HEADING.exec(line);
    if (fm) {
      flush();
      current = { id: fm[1], title: fm[2], lines: [] };
    } else if (current) current.lines.push(line);
    else head.push(line);
  }
  flush();
  // A slot with no families (R, S) holds its table directly: one family named after the slot.
  if (families.length === 0) {
    families.push({
      id: code,
      title,
      intro: '',
      ideas: tableRows(head).map((r) => parseIdea(r, code, code)),
    });
  }
  return { code, title, intro: intro(head), families };
}

function parseGrid(lines: string[]): StructureGrid {
  const header = lines.find(isRow);
  const columns = header ? cells(header).slice(1) : [];
  const rows = tableRows(lines).map(([label, ...rest]) => ({
    label,
    cells: rest.map((c) => {
      const m = GRID_CELL.exec(c);
      if (!m) throw new Error(`inventory structure grid, ${label}: cannot read cell "${c}"`);
      return { name: m[1], setting: m[2] ?? null, status: m[3] as IdeaStatus };
    }),
  }));
  return { columns, rows };
}

export function parseInventory(md: string): Inventory {
  const inv: Inventory = { slots: [], grid: { columns: [], rows: [] } };
  for (const { heading, lines } of sections(md)) {
    const sm = SLOT_HEADING.exec(heading);
    if (sm) inv.slots.push(parseSlot(sm[1], sm[2], lines));
    else if (heading === '## Structure grid') inv.grid = parseGrid(lines);
  }
  return inv;
}

/** Stable DOM id for an idea's row, so a grid cell can jump to it. */
export const ideaAnchor = (name: string) =>
  'idea-' + name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');

