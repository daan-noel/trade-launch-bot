import { InfoTooltip } from 'components/ui/InfoTooltip';
import { cn } from 'lib/cn';
import type { Help } from './summaryHelp';

/** The page's fixed terms, each with the one style it wears everywhere. */
const TERM_CLASS: Record<string, string> = {
  'Target IXs': 'text-primary',
  'Any IXs': 'text-accent',
  'selected IXs': 'text-text',
  'In pool': 'text-text',
  pool: 'text-text',
  'Pass filters': 'text-text',
  'any wallet': 'text-text',
  'at a point': 'text-primary',
  'not right after': 'text-text',
  'right after': 'text-text',
  'never bought': 'text-text',
  'passes the filters': 'text-text',
  'filters pass': 'text-text',
  'filters fail': 'text-accent',
};
// Longer terms first: `not right after` before `right after`, `In pool` before `pool`.
const TERM_RE = new RegExp(
  `\\b(${Object.keys(TERM_CLASS)
    .sort((x, y) => y.length - x.length)
    .join('|')})\\b`,
);

/** One of the fixed terms, standing out from the text around it. */
export function Term({ children, className }: { children: string; className?: string }) {
  return <span className={cn('font-semibold', TERM_CLASS[children], className)}>{children}</span>;
}

/** A sentence with its fixed terms styled. */
export function Terms({ text }: { text: string }) {
  // A split on a capturing group keeps the matches, at the odd indexes.
  return (
    <>
      {text.split(new RegExp(TERM_RE, 'g')).map((part, i) => (i % 2 ? <Term key={i}>{part}</Term> : part))}
    </>
  );
}

/** A help's labelled lines: the label in its own column, one idea per line. */
function HelpLines({ help }: { help: Help }) {
  return (
    <span className="grid grid-cols-[auto_1fr] gap-x-2 gap-y-1">
      {help.lines.map((l) => (
        <span key={l.label} className="contents">
          <span className="pt-px text-[9px] font-bold uppercase tracking-wider text-text-dim/70">{l.label}</span>
          <span className="text-text-mid">
            <Terms text={l.text} />
          </span>
        </span>
      ))}
    </span>
  );
}

/** The info popover of a summary row, table or filter line. */
export function HelpTip({ title, help, className }: { title: string; help: Help; className?: string }) {
  return (
    <InfoTooltip title={title} figure={help.figure} className={className}>
      <HelpLines help={help} />
    </InfoTooltip>
  );
}
