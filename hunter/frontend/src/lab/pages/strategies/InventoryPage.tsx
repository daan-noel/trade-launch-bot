import { Fragment, useMemo, useState } from 'react';
import { PageHeader } from 'components/ui/PageHeader';
import { cn } from 'lib/cn';
// The one source: the page parses the markdown at load and HMR-reloads on every save.
import inventoryMd from '../../../../../docs/plans/strategies/_!___inventory.md?raw';
import { STATUSES, ideaAnchor, parseInventory, type IdeaStatus } from '@lab/lib/inventory';

/** One look per status: filled for a read idea, dashed for one never tried. */
const STATUS_BOX: Record<IdeaStatus, string> = {
  keep: 'border-green/50 bg-green/15 text-green',
  open: 'border-warning/50 bg-warning/12 text-warning',
  red: 'border-red/50 bg-red/12 text-red',
  dead: 'border-white/15 bg-white/5 text-text-dim',
  new: 'border-dashed border-white/25 text-text-mid',
};

const STATUS_LABEL: Record<IdeaStatus, string> = {
  keep: 'keep',
  open: 'open',
  red: 'red',
  dead: 'dead',
  new: 'new (never tried)',
};

/** Inline markdown: `**bold**`, `` `code` ``. */
function Md({ text }: { text: string }) {
  return (
    <>
      {text.split(/(\*\*[^*]+\*\*|`[^`]+`)/g).map((p, i) =>
        p.startsWith('**') ? (
          <strong key={i} className="text-text">
            {p.slice(2, -2)}
          </strong>
        ) : p.startsWith('`') ? (
          <code key={i} className="rounded bg-white/6 px-1 font-mono">
            {p.slice(1, -1)}
          </code>
        ) : (
          p
        ),
      )}
    </>
  );
}

function StatusChip({ status }: { status: IdeaStatus }) {
  return (
    <span className={cn('inline-block rounded border px-1.5 py-0.5 text-[11px] font-bold', STATUS_BOX[status])}>
      {status}
    </span>
  );
}

/**
 * The idea inventory (lab), rendered from `_!___inventory.md`: the structure grid
 * first (every ix-structure idea by what it reads and how structures are grouped),
 * then every idea in one table - name, status, idea, example - with the rest of
 * the row a click away.
 */
export function InventoryPage() {
  const inv = useMemo(() => parseInventory(inventoryMd), []);
  const [open, setOpen] = useState<Set<string>>(new Set());
  const total = inv.slots.reduce((n, s) => n + s.families.reduce((m, f) => m + f.ideas.length, 0), 0);

  const toggle = (name: string) =>
    setOpen((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });

  const jumpTo = (name: string) => {
    setOpen((prev) => new Set(prev).add(name));
    requestAnimationFrame(() =>
      document.getElementById(ideaAnchor(name))?.scrollIntoView({ behavior: 'smooth', block: 'center' }),
    );
  };

  const cell = 'px-3 py-2 align-top';

  return (
    <div className="pt-2">
      <PageHeader title="Idea inventory" description={`${total} ideas, from _!___inventory.md`} />

      <div className="mb-4 flex flex-wrap items-center gap-2 text-xs text-text-dim">
        Status:
        {STATUSES.map((s) => (
          <span key={s} className={cn('rounded border px-2 py-0.5 font-bold', STATUS_BOX[s])}>
            {STATUS_LABEL[s]}
          </span>
        ))}
      </div>

      {inv.grid.rows.length > 0 && (
        <section className="mb-8">
          <h2 className="mb-1 text-base font-extrabold text-text">Structure grid</h2>
          <p className="mb-3 text-xs text-text-dim">
            Every ix-structure idea: what we read, by how the structures are grouped. Click a cell to open its row.
          </p>
          <div className="overflow-x-auto">
            <table className="w-full min-w-200 border-separate border-spacing-1.5 text-[13px]">
              <thead>
                <tr className="text-left text-xs text-text-dim">
                  <th className="w-44 px-2 font-semibold" />
                  {inv.grid.columns.map((c) => (
                    <th key={c} className="px-2 font-semibold capitalize">
                      {c}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {inv.grid.rows.map((r) => (
                  <tr key={r.label}>
                    <td className="px-2 align-middle text-xs font-bold capitalize text-text">{r.label}</td>
                    {r.cells.map((c, i) => (
                      <td key={i} className="p-0">
                        <button
                          type="button"
                          onClick={() => jumpTo(c.name)}
                          className={cn(
                            'h-full w-full rounded-md border px-2.5 py-1.5 text-left transition hover:brightness-125',
                            STATUS_BOX[c.status],
                          )}
                        >
                          <span className="block font-bold">{c.name}</span>
                          <span className="block text-[11px] opacity-80">
                            {c.setting ? `${c.setting} - ` : ''}
                            {c.status}
                          </span>
                        </button>
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>
      )}

      <h2 className="mb-2 text-base font-extrabold text-text">All ideas</h2>
      <div className="overflow-x-auto rounded-lg border border-white/8">
        <table className="w-full min-w-225 text-[13px] leading-relaxed">
          <thead className="text-left text-xs text-text-dim">
            <tr>
              <th className="sticky top-0 w-52 bg-bg-panel px-3 py-2 font-semibold">Name</th>
              <th className="sticky top-0 w-20 bg-bg-panel px-3 py-2 font-semibold">Status</th>
              <th className="sticky top-0 bg-bg-panel px-3 py-2 font-semibold">Idea</th>
              <th className="sticky top-0 w-104 bg-bg-panel px-3 py-2 font-semibold">Example</th>
            </tr>
          </thead>
          <tbody>
            {inv.slots.map((s) => (
              <Fragment key={s.code}>
                <tr className="border-t border-white/10 bg-primary/10">
                  <td colSpan={4} className="px-3 py-2 text-sm font-extrabold text-primary">
                    {s.code} {s.title}
                  </td>
                </tr>
                {s.families.map((f) => (
                  <Fragment key={f.id}>
                    {f.id !== s.code && (
                      <tr className="border-t border-white/8 bg-white/4">
                        <td colSpan={4} className="px-3 py-1.5 text-xs font-bold text-text">
                          {f.id} {f.title}
                        </td>
                      </tr>
                    )}
                    {f.ideas.map((i) => {
                      const isOpen = open.has(i.name);
                      return (
                        <Fragment key={i.name}>
                          <tr
                            id={ideaAnchor(i.name)}
                            onClick={() => toggle(i.name)}
                            className={cn(
                              'cursor-pointer border-t border-white/6 text-text-mid hover:bg-white/3',
                              isOpen && 'bg-white/3',
                            )}
                          >
                            <td className={cn(cell, 'font-bold text-text')}>
                              <span className="mr-1.5 text-text-dim">{isOpen ? '-' : '+'}</span>
                              {i.name}
                            </td>
                            <td className={cell}>
                              <StatusChip status={i.status} />
                            </td>
                            <td className={cell}>
                              <Md text={i.idea} />
                            </td>
                            <td className={cell}>
                              <ol className="flex flex-col gap-0.5">
                                {i.example.map((step, n) => (
                                  <li key={n}>
                                    <Md text={step} />
                                  </li>
                                ))}
                              </ol>
                            </td>
                          </tr>
                          {isOpen && (
                            <tr className="bg-white/3 text-text-mid">
                              <td colSpan={4} className="px-3 pb-3 pt-1">
                                <dl className="grid gap-x-6 gap-y-2 md:grid-cols-2">
                                  {(
                                    [
                                      ['Meaning', i.meaning],
                                      ['Why it matters', i.why],
                                      ['Parameters', i.parameters || '-'],
                                      ['Tested in', i.books.join(', ') || '-'],
                                    ] as const
                                  ).map(([label, text]) => (
                                    <div key={label}>
                                      <dt className="text-[10px] font-semibold uppercase tracking-wide text-text-dim">
                                        {label}
                                      </dt>
                                      <dd>
                                        <Md text={text} />
                                      </dd>
                                    </div>
                                  ))}
                                </dl>
                              </td>
                            </tr>
                          )}
                        </Fragment>
                      );
                    })}
                  </Fragment>
                ))}
              </Fragment>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
