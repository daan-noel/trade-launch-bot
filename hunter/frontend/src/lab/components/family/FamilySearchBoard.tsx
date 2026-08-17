import type { ReactNode } from 'react';

import { Badge } from 'components/ui/Badge';
import { IconButton } from 'components/ui/IconButton';
import { SimulateIcon, SpinnerIcon } from 'components/ui/icons';
import { InlineAlert } from 'components/ui/Modal';
import { LabelTip } from 'components/strategy/LabelTip';
import { ruleParamsCell } from 'components/strategy/RuleParamsSummary';
import type { HelpTip } from 'lib/strategy/strategyHelp';
import { cn } from 'lib/cn';
import { familyVerdict, type FamilyGate } from '@lab/lib/familySearchVerdict';
import type {
  FamilyAlarmRow,
  FamilyCandidateRow,
  FamilySearchReport,
} from '@lab/lib/familySearchTypes';

/**
 * The family-search board — one screen that answers "should I promote this?".
 *
 * Order is the argument: the verdict and its four gates, then the portrait (the
 * product — the draft is only its executable form), then the grade, then the
 * evidence each part of the grade came from. Nothing is collapsed: this page is
 * read once per run and every section is part of the same decision.
 *
 * The one presentation rule the payload imposes: **`fit_ret_pct` ranks and
 * `target_ret_pct` reports.** Every fit number on this board is dimmed and
 * labelled `rank only`, because on the reference family every candidate is
 * negative on the fit set while the winner pays +31% on the held-out cohort.
 */
export function FamilySearchBoard({
  report,
  onPromote,
  onSimulate,
  simBusy,
}: {
  report: FamilySearchReport;
  onPromote: (row: FamilyCandidateRow, label: string) => void;
  onSimulate: () => void;
  simBusy: boolean;
}) {
  const v = familyVerdict(report);
  const { draft, ungated_control: ungated, incumbent, capture } = report;

  return (
    <div className="space-y-8">
      {/* ── The answer ───────────────────────────────────────────────────── */}
      <section
        className={cn(
          'rounded-xl border p-4',
          v.tone === 'success' && 'border-green/30 bg-green/5',
          v.tone === 'warning' && 'border-warning/30 bg-warning/5',
          v.tone === 'danger' && 'border-red/30 bg-red/5',
        )}
      >
        <div className="flex flex-wrap items-center gap-2">
          <Badge variant={v.tone} size="md">
            {v.label}
          </Badge>
          <span className="text-xs text-text-dim">
            {report.fingerprint_name}
            {report.family.varied_axis && (
              <> · family varies <code className="text-text-mid">{report.family.varied_axis}</code></>
            )}
          </span>
        </div>
        <p className="mt-2 text-lg leading-snug text-text">{v.headline}</p>
        <p className="mt-1 max-w-3xl text-sm leading-relaxed text-text-mid">{v.body}</p>

        <div className="mt-3 grid gap-x-6 gap-y-1.5 border-t border-white/10 pt-3 sm:grid-cols-2">
          {v.gates.map((g) => (
            <GateLine key={g.key} gate={g} />
          ))}
        </div>
      </section>

      {/* ── The portrait: the product ────────────────────────────────────── */}
      {report.portrait.length > 0 && (
        <Section
          title="What this launch shape does"
          caption="The finding in words. The draft below is the same thing, executable."
        >
          <ul className="max-w-4xl space-y-2">
            {report.portrait.map((line, i) => (
              <li key={i} className="flex gap-2.5 text-sm leading-relaxed text-text-mid">
                <span className="select-none pt-0.5 text-text-dim/60">▸</span>
                <span>{line}</span>
              </li>
            ))}
          </ul>
        </Section>
      )}

      {report.diagnostics.map((d) => (
        <InlineAlert key={d} variant="warning">
          {d}
        </InlineAlert>
      ))}

      {/* ── Can this shape pay for execution at all? ─────────────────────── */}
      {(report.cost_clearance || report.spread) && (
        <Section
          title="Execution"
          caption="Asked before the search and again of its answer. A launch shape whose available moves live inside the round trip cannot be traded at this buy size — the loss is a ratio, and no threshold changes a ratio."
        >
          <div className="grid gap-3 lg:grid-cols-2">
            {report.cost_clearance && (
              <GradeCard
                tone="neutral"
                title="Cost clearance"
                tip={TIPS.clearance}
                blurb="The typical entry's BEST available exit, against what one round trip costs. Measured with no rule at all."
                value={
                  report.cost_clearance.headroom != null
                    ? `${report.cost_clearance.headroom.toFixed(1)}x`
                    : null
                }
                emptyHint="The cohort's available upside was not measurable on this run."
                stats={[
                  {
                    label: 'best exit (median)',
                    value: pctText(report.cost_clearance.median_move_pct),
                  },
                  { label: 'round trip', value: `${fmt(report.cost_clearance.band_pct, 2)}%` },
                  {
                    label: 'priced',
                    value: `${report.cost_clearance.n_priced} · ${report.cost_clearance.n_with_upside} with upside`,
                  },
                ]}
                footer={
                  <p
                    className={cn(
                      'text-[11px] leading-snug',
                      report.cost_clearance.refused
                        ? 'text-red'
                        : report.cost_clearance.thin
                          ? 'text-warning'
                          : 'text-text-dim',
                    )}
                  >
                    {report.cost_clearance.refused
                      ? 'refused — no search was run'
                      : report.cost_clearance.thin
                        ? 'clears by less than one round trip'
                        : `bar: ${report.cost_clearance.margin.toFixed(1)}x the round trip`}
                  </p>
                }
              />
            )}
            {report.spread && (
              <GradeCard
                tone="neutral"
                title="Fill spread"
                tip={TIPS.spread}
                blurb="The same closes, repriced at the friendliest honest fill. The gap is what execution costs, not what the signal is worth."
                value={ppText(report.spread.spread_pp)}
                emptyHint="No spread on this run."
                stats={[
                  { label: 'this run', value: pctText(report.spread.authority_ret_pct) },
                  { label: 'optimistic', value: pctText(report.spread.optimistic_ret_pct) },
                  { label: 'closes', value: String(report.spread.n_common) },
                ]}
                footer={
                  <p
                    className={cn(
                      'text-[11px] leading-snug',
                      report.spread.fill_luck ? 'text-warning' : 'text-text-dim',
                    )}
                  >
                    {report.spread.fill_luck
                      ? 'the swing is larger than the edge — priced on fill luck'
                      : 'the edge survives its own execution swing'}
                    {!report.spread.clean &&
                      ` · taken sets differ (${report.spread.n_authority_only} / ${report.spread.n_optimistic_only})`}
                  </p>
                }
              />
            )}
          </div>
        </Section>
      )}

      {/* ── The grade ────────────────────────────────────────────────────── */}
      <Section
        title="The grade"
        caption="What the draft pays on the held-out cohort, against the two things that exist before any rule does: buying everything, and the best exit that was actually available."
      >
        <div className="grid gap-3 lg:grid-cols-3">
          <GradeCard
            tone="primary"
            title="Draft"
            tip={TIPS.draft}
            blurb="The finalist, replayed on the held-out target cohort."
            value={draft ? pctText(draft.target_ret_pct) : null}
            emptyHint="No candidate survived the fit."
            stats={
              draft
                ? [
                    { label: 'PnL', value: `${fmt(draft.target_pnl_sol, 3)} ◎` },
                    { label: 'entered', value: `${pct01(draft.target_enter_pct)} of matched` },
                    { label: 'tokens', value: String(draft.target_n_tokens) },
                  ]
                : []
            }
            footer={
              draft ? (
                <p className="text-[10px] leading-tight text-text-dim">
                  fit {pctText(draft.fit_ret_pct)}{' '}
                  <span className="text-text-dim/70">· rank only, never a level</span>
                </p>
              ) : null
            }
            actions={
              draft ? (
                <div className="mt-3 flex flex-wrap items-center gap-1.5">
                  <button
                    type="button"
                    className="rounded border border-white/20 px-2 py-0.5 text-[11px] text-text-mid transition hover:bg-white/5"
                    onClick={() => onPromote(draft, 'draft')}
                    title="Save as an inactive paper rule"
                  >
                    Promote…
                  </button>
                  <IconButton
                    variant="ghost"
                    size="sm"
                    onClick={onSimulate}
                    disabled={simBusy}
                    label={simBusy ? 'Simulating…' : 'Simulate'}
                    title="Full Simulate of the unsaved draft — same range, fill, cost and copycat as this form"
                  >
                    {simBusy ? <SpinnerIcon /> : <SimulateIcon />}
                  </IconButton>
                </div>
              ) : null
            }
          />

          <GradeCard
            tone="neutral"
            title="Ungated control"
            tip={TIPS.ungated}
            blurb="Buy every matched token, same exit bag. A property of the cohort, not a rule."
            value={ungated ? pctText(ungated.target_ret_pct) : null}
            emptyHint="No ungated control on this run."
            stats={
              ungated
                ? [
                    { label: 'PnL', value: `${fmt(ungated.target_pnl_sol, 3)} ◎` },
                    { label: 'tokens', value: String(ungated.target_n_tokens) },
                  ]
                : []
            }
            footer={
              v.edgePp != null ? (
                <p
                  className={cn(
                    'text-[11px] leading-tight',
                    v.edgePp > 0 ? 'text-green' : 'text-red',
                  )}
                >
                  the gate is worth {ppText(v.edgePp)}
                </p>
              ) : null
            }
          />

          <GradeCard
            tone="neutral"
            title="Oracle capture"
            tip={TIPS.capture}
            blurb="Of the money that was available after each fill, how much the exit took."
            value={capture.capture_pct != null ? `${capture.capture_pct.toFixed(0)}%` : null}
            emptyHint="No entry had a profitable exit available — that grades the entry, not the exit."
            stats={[
              { label: 'had upside', value: String(capture.n_with_upside) },
              { label: 'never did', value: String(capture.n_no_upside) },
              {
                label: 'took / available',
                value: `${fmt(capture.realized_pnl_sol, 3)} / ${fmt(capture.oracle_pnl_sol, 3)} ◎`,
              },
            ]}
            footer={
              <p className="text-[10px] leading-tight text-text-dim">
                the second count grades the <span className="text-text-mid">entry</span> — those
                tokens had nothing for any exit to take
              </p>
            }
          />
        </div>

        {incumbent && (
          <div className="mt-3 rounded-lg border border-dashed border-white/12 bg-white/2 p-3">
            <div className="flex flex-wrap items-baseline gap-2">
              <LabelTip tip={TIPS.incumbent}>
                <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
                  Incumbent
                </span>
              </LabelTip>
              <Badge variant="neutral" size="sm">
                display only
              </Badge>
              <span className="font-mono text-sm text-text-mid">
                {pctText(incumbent.target_ret_pct)}
              </span>
              <span className="font-mono text-[11px] text-text-dim">
                {fmt(incumbent.target_pnl_sol, 3)} ◎ · {incumbent.target_n_tokens} tokens
              </span>
            </div>
            <p className="mt-1 text-[11px] text-text-dim">
              Your saved rule, replayed on the same cohort with the same buy size, fill and cost.
              It supplied nothing to the search — no threshold, no structure, no buy size, no cap.
            </p>
          </div>
        )}
      </Section>

      {/* ── The draft itself ─────────────────────────────────────────────── */}
      {draft && (
        <Section
          title="The draft rule"
          caption="Promote writes exactly these clauses; Simulate runs them unsaved."
        >
          <div className="mb-2 flex flex-wrap items-center gap-1.5">
            {draft.families.map((f) => (
              <Badge key={f} variant="info" size="sm" title="End-event family this exit draws on">
                {f}
              </Badge>
            ))}
            {draft.flags.map((f) => (
              <Badge key={f} variant="warning" size="sm" title={f}>
                flagged
              </Badge>
            ))}
          </div>
          {draft.flags.map((f) => (
            <p key={f} className="mb-2 text-[11px] leading-snug text-warning">
              {f}
            </p>
          ))}
          {ruleParamsCell(draft.params)}
        </Section>
      )}

      {/* ── What was searched ────────────────────────────────────────────── */}
      <Section
        title="The family"
        caption="Fingerprints identical on every axis but one. The ordering pooled the fit rows; the reported level comes from the target row alone."
      >
        <TableShell>
          <thead className="text-[10px] uppercase tracking-wide text-text-dim">
            <tr>
              <Th>Fingerprint</Th>
              <Th>Role</Th>
              <Th align="right">{report.family.varied_axis ?? 'Axis'}</Th>
              <Th align="right" tip={TIPS.nMatched}>
                Matched
              </Th>
              <Th align="right" tip={TIPS.cohortUngated}>
                Ungated return
              </Th>
            </tr>
          </thead>
          <tbody>
            {report.family.members.map((m) => (
              <tr
                key={m.fp_id}
                className={cn('border-t border-white/6', m.is_target && 'bg-primary/5')}
              >
                <Td>
                  <span className={m.is_target ? 'text-text' : 'text-text-mid'}>{m.name}</span>
                </Td>
                <Td>
                  {m.is_target ? (
                    <Badge variant="primary" size="sm" title="Held out — the reported level comes from this cohort">
                      target
                    </Badge>
                  ) : (
                    <span className="text-[11px] text-text-dim">fit</span>
                  )}
                </Td>
                <Td align="right" mono>
                  {m.axis_value == null ? '—' : fmt(m.axis_value, 2)}
                </Td>
                <Td align="right" mono>
                  {m.n_matched}
                </Td>
                <Td align="right" mono>
                  {m.ungated_ret_pct == null ? '—' : pctText(m.ungated_ret_pct)}
                </Td>
              </tr>
            ))}
          </tbody>
        </TableShell>
        <p className="mt-2 text-[11px] leading-snug text-text-dim">
          Check <span className="text-text-mid">Matched</span> against a hand count every run — an
          ix-labels-only approximation of one reference cohort takes 3,440 tokens where the engine
          takes 264, and two rules invert in rank between the two populations.
        </p>
      </Section>

      {/* ── Where the money came out ─────────────────────────────────────── */}
      {report.attribution.length > 0 && (
        <Section
          title="Which alarm made the money"
          caption="Straight from the engine's own exit reason — no re-runs, no ablation. Ranked by money, because a term that fires 200× for a small loss outranks a winner on count alone."
        >
          <TableShell>
            <thead className="text-[10px] uppercase tracking-wide text-text-dim">
              <tr>
                <Th>Exit term</Th>
                <Th align="right">Closed</Th>
                <Th align="right" tip={TIPS.level}>
                  Asked → got
                </Th>
                <Th align="right">PnL ◎</Th>
                <Th align="right" tip={TIPS.pnlPct}>
                  Return
                </Th>
              </tr>
            </thead>
            <tbody>
              {[...report.attribution]
                .sort((a, b) => b.pnl_sol - a.pnl_sol)
                .map((a) => (
                  <tr key={a.slot} className="border-t border-white/6">
                    <Td>
                      <span className="font-mono text-text-mid">
                        {a.label ?? `slot ${a.slot} (unnamed)`}
                      </span>
                    </Td>
                    <Td align="right" mono>
                      {a.n}
                    </Td>
                    <Td align="right" mono>
                      <LevelCell row={a} />
                    </Td>
                    <Td align="right" mono tone={a.pnl_sol < 0 ? 'bad' : 'good'}>
                      {fmt(a.pnl_sol, 3)}
                    </Td>
                    <Td align="right" mono tone={a.pnl_pct < 0 ? 'bad' : 'good'}>
                      {pctText(a.pnl_pct)}
                    </Td>
                  </tr>
                ))}
              {report.attribution_other_n > 0 && (
                <tr className="border-t border-white/6 text-text-dim">
                  <Td>
                    <span title="Take-profit, stop, timeout, death — closes that were not authored metric exits">
                      everything else
                    </span>
                  </Td>
                  <Td align="right" mono>
                    {report.attribution_other_n}
                  </Td>
                  <Td align="right">—</Td>
                  <Td align="right" mono>
                    {fmt(report.attribution_other_pnl_sol, 3)}
                  </Td>
                  <Td align="right">—</Td>
                </tr>
              )}
            </tbody>
          </TableShell>
        </Section>
      )}

      {/* ── Narrow re-check ──────────────────────────────────────────────── */}
      {report.narrow_recheck.length > 0 && (
        <Section
          title="What each term is worth here"
          caption="The draft re-scored on the target cohort with one term dropped. A broad fit is blind to a term that only one cohort needs, which is what this catches."
        >
          <TableShell>
            <thead className="text-[10px] uppercase tracking-wide text-text-dim">
              <tr>
                <Th>Dropped term</Th>
                <Th align="right">With it</Th>
                <Th align="right">Without it</Th>
                <Th align="right">Δ</Th>
                <Th />
              </tr>
            </thead>
            <tbody>
              {report.narrow_recheck.map((t) => (
                <tr key={t.label} className="border-t border-white/6">
                  <Td>
                    <span className="font-mono text-text-mid">{t.label}</span>
                  </Td>
                  <Td align="right" mono>
                    {pctText(t.ret_full_pct)}
                  </Td>
                  <Td align="right" mono>
                    {pctText(t.ret_without_pct)}
                  </Td>
                  <Td align="right" mono tone={t.delta_pct < 0 ? 'bad' : 'good'}>
                    {ppText(t.delta_pct)}
                  </Td>
                  <Td>
                    {t.inert && (
                      <Badge
                        variant="neutral"
                        size="sm"
                        title="Dropping it changed nothing at all on this cohort — dead weight here"
                      >
                        inert
                      </Badge>
                    )}
                  </Td>
                </tr>
              ))}
            </tbody>
          </TableShell>
          <p className="mt-2 text-[11px] text-text-dim">
            Δ is the draft minus the version without that term: positive means the term earns its
            place, negative means it costs money.
          </p>
        </Section>
      )}

      {/* ── Entry timing ─────────────────────────────────────────────────── */}
      {report.entry_timing.length > 0 && (
        <Section
          title="What each entry clause does to the timing"
          caption="The draft re-run without each entry clause. A clause that holds entries back AND whose kept entries have less upside left is created by the move it is trying to precede — it selects moments after the move, not before it."
        >
          <TableShell>
            <thead className="text-[10px] uppercase tracking-wide text-text-dim">
              <tr>
                <Th>Entry clause</Th>
                <Th align="right" tip={TIPS.delay}>
                  Delay added
                </Th>
                <Th align="right" tip={TIPS.captureDelta}>
                  Capture
                </Th>
                <Th align="right">Entries filtered</Th>
                <Th>Verdict</Th>
              </tr>
            </thead>
            <tbody>
              {report.entry_timing.map((t) => (
                <tr key={t.clause} className="border-t border-white/6">
                  <Td>
                    <span className="font-mono text-text-mid">{t.clause}</span>
                  </Td>
                  <Td align="right" mono tone={t.delay_added_secs > 0 ? 'bad' : 'good'}>
                    {`${signed(t.delay_added_secs, 1)}s`}
                  </Td>
                  <Td
                    align="right"
                    mono
                    tone={
                      t.capture_delta_pp != null && t.capture_delta_pp < 0 ? 'bad' : 'good'
                    }
                  >
                    {t.capture_delta_pp == null ? '—' : ppText(t.capture_delta_pp)}
                  </Td>
                  <Td align="right" mono>
                    {ppText(t.admit_delta_pct)}
                  </Td>
                  <Td>
                    {t.lagging ? (
                      <span className="inline-flex flex-col gap-0.5">
                        <Badge variant="warning" size="sm">
                          lagging
                        </Badge>
                        {t.note && (
                          <span className="text-[11px] leading-snug text-text-dim">{t.note}</span>
                        )}
                      </span>
                    ) : (
                      <span className="text-[11px] text-text-dim">
                        {t.delay_added_secs > 0 ? 'waits, and the wait pays' : 'does not bind timing'}
                      </span>
                    )}
                  </Td>
                </tr>
              ))}
            </tbody>
          </TableShell>
          <p className="mt-2 text-[11px] leading-snug text-text-dim">
            Every column is the draft minus that clause: a positive delay means the clause pushes
            the entry later, and a negative capture means the entries it keeps took less of the
            money that was available. Diagnostic only — waiting for confirmation is a legitimate
            edge, and it is the pair that makes a finding.
          </p>
        </Section>
      )}

      {/* ── Entry gates ──────────────────────────────────────────────────── */}
      {report.entry_gates.length > 0 && (
        <Section
          title="Entry clauses vs the varied axis"
          caption="An entry clause whose admit rate tracks the axis the family varies is the fingerprint read twice, not a filter within it — and it looks like a working entry rule on every family that varies that axis."
        >
          <TableShell>
            <thead className="text-[10px] uppercase tracking-wide text-text-dim">
              <tr>
                <Th>Clause</Th>
                <Th align="right" tip={TIPS.gateRho}>
                  rho vs axis
                </Th>
                <Th>Verdict</Th>
              </tr>
            </thead>
            <tbody>
              {report.entry_gates.map((g) => (
                <tr key={g.clause} className="border-t border-white/6">
                  <Td>
                    <span className="font-mono text-text-mid">{g.clause}</span>
                  </Td>
                  <Td align="right" mono>
                    {g.rho == null ? '—' : signed(g.rho, 2)}
                  </Td>
                  <Td>
                    {g.refused ? (
                      <span className="inline-flex flex-col gap-0.5">
                        <Badge variant="danger" size="sm">
                          refused
                        </Badge>
                        {g.reason && (
                          <span className="text-[11px] leading-snug text-text-dim">{g.reason}</span>
                        )}
                      </span>
                    ) : (
                      <span className="text-[11px] text-text-dim">
                        {g.rho == null ? 'not measurable on one cohort' : 'stands'}
                      </span>
                    )}
                  </Td>
                </tr>
              ))}
            </tbody>
          </TableShell>
        </Section>
      )}

      {/* ── Archive ──────────────────────────────────────────────────────── */}
      {report.archive.length > 0 && (
        <Section
          title="Archive"
          caption="Every candidate in fit order — row 1 is the draft. Fit ranks; target reports."
          right={
            <span className="text-[11px] text-text-dim">
              {report.library.n_candidates} candidates
              {report.library.dropped_by_quota > 0 &&
                ` · ${report.library.dropped_by_quota} turned away by the per-family quota`}
            </span>
          }
        >
          {report.library.by_family.length > 0 && (
            <div className="mb-2 flex flex-wrap items-center gap-1.5">
              <span className="text-[11px] text-text-dim">end-event families searched:</span>
              {report.library.by_family.map(([f, n]) => (
                <Badge key={f} variant="neutral" size="sm">
                  {f} · {n}
                </Badge>
              ))}
            </div>
          )}
          <TableShell>
            <thead className="text-[10px] uppercase tracking-wide text-text-dim">
              <tr>
                <Th>#</Th>
                <Th>Params</Th>
                <Th align="right" tip={TIPS.fitCol}>
                  Fit (rank only)
                </Th>
                <Th align="right" tip={TIPS.targetCol}>
                  Target return
                </Th>
                <Th align="right">◎</Th>
                <Th align="right">Enter%</Th>
                <Th />
              </tr>
            </thead>
            <tbody>
              {report.archive.map((row, i) => (
                <tr key={row.key} className={cn('border-t border-white/6', i === 0 && 'bg-primary/5')}>
                  <Td mono>{i + 1}</Td>
                  <Td>
                    {ruleParamsCell(row.params)}
                    {row.flags.length > 0 && (
                      <span className="ml-1 align-middle">
                        <Badge variant="warning" size="sm" title={row.flags.join(' · ')}>
                          flagged
                        </Badge>
                      </span>
                    )}
                  </Td>
                  <Td align="right" mono>
                    <span className="text-text-dim">{pctText(row.fit_ret_pct)}</span>
                  </Td>
                  <Td align="right" mono tone={row.target_ret_pct < 0 ? 'bad' : 'good'}>
                    {pctText(row.target_ret_pct)}
                  </Td>
                  <Td align="right" mono>
                    {fmt(row.target_pnl_sol, 3)}
                  </Td>
                  <Td align="right" mono>
                    {pct01(row.target_enter_pct)}
                  </Td>
                  <Td>
                    <button
                      type="button"
                      className="rounded border border-white/20 px-2 py-0.5 text-[11px] text-text-mid transition hover:bg-white/5"
                      onClick={() => onPromote(row, i === 0 ? 'draft' : `archive ${i + 1}`)}
                    >
                      Promote…
                    </button>
                  </Td>
                </tr>
              ))}
            </tbody>
          </TableShell>
        </Section>
      )}
    </div>
  );
}

// ── Bits ────────────────────────────────────────────────────────────────────

function GateLine({ gate }: { gate: FamilyGate }) {
  const mark = gate.ok == null ? '·' : gate.ok ? '✓' : '✕';
  return (
    <p className="flex gap-2 text-[11px] leading-snug">
      <span
        className={cn(
          'w-3 shrink-0 text-center font-bold',
          gate.ok == null && 'text-text-dim',
          gate.ok === true && 'text-green',
          gate.ok === false && 'text-red',
        )}
        aria-hidden
      >
        {mark}
      </span>
      <span>
        <span className="text-text-mid">{gate.label}</span>
        <span className="text-text-dim"> — {gate.detail}</span>
      </span>
    </p>
  );
}

/**
 * The authored threshold against the level the term actually closed at — printed as a
 * pair only where the two are the same quantity. `pnl <= -8` realizing −20 is a stop
 * that does not stop; `stall >= 30` has no comparable realized number at all.
 */
function LevelCell({ row }: { row: FamilyAlarmRow }) {
  if (row.authored_level == null) return <span className="text-text-dim">—</span>;
  if (!row.level_is_return || row.realized_level_pct == null) {
    return <span className="text-text-dim">{fmt(row.authored_level, 2)}</span>;
  }
  const over = row.level_overshoot_pp ?? 0;
  return (
    <span
      className="inline-flex items-baseline gap-1"
      title={`Authored ${fmt(row.authored_level, 1)}%, actually closed at ${fmt(
        row.realized_level_pct,
        1,
      )}% gross — ${Math.abs(over).toFixed(1)} points ${over < 0 ? 'worse' : 'better'} than the level it asked for.`}
    >
      <span className="text-text-dim">{fmt(row.authored_level, 1)}</span>
      <span className="text-text-dim/60">→</span>
      <span className={over < -1 ? 'text-red' : 'text-text-mid'}>
        {fmt(row.realized_level_pct, 1)}
      </span>
    </span>
  );
}

function Section({
  title,
  caption,
  right,
  children,
}: {
  title: string;
  caption: string;
  right?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section>
      <div className="mb-2 flex flex-wrap items-baseline justify-between gap-2">
        <h2 className="text-sm font-semibold text-text">{title}</h2>
        {right}
      </div>
      <p className="mb-3 max-w-4xl text-[11px] leading-relaxed text-text-dim">{caption}</p>
      {children}
    </section>
  );
}

function GradeCard({
  tone,
  title,
  tip,
  blurb,
  value,
  emptyHint,
  stats,
  footer,
  actions,
}: {
  tone: 'primary' | 'neutral';
  title: string;
  tip: HelpTip;
  blurb: string;
  value: string | null;
  emptyHint: string;
  stats: { label: string; value: string }[];
  footer?: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <div
      className={cn(
        'flex flex-col rounded-lg border p-3',
        tone === 'primary' ? 'border-primary/25 bg-primary/5' : 'border-white/8 bg-white/2',
      )}
    >
      <h3 className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
        <LabelTip tip={tip}>{title}</LabelTip>
      </h3>
      <p className="mb-2 text-[11px] leading-snug text-text-dim">{blurb}</p>
      {value == null ? (
        <p className="text-xs text-text-dim">{emptyHint}</p>
      ) : (
        <>
          <p className="font-mono text-2xl leading-tight text-text">{value}</p>
          <div className="mt-1.5 flex flex-wrap gap-x-3 gap-y-1 text-[11px]">
            {stats.map((s) => (
              <span key={s.label} className="inline-flex items-baseline gap-1">
                <span className="text-text-dim/80">{s.label}</span>
                <span className="font-mono text-text-mid">{s.value}</span>
              </span>
            ))}
          </div>
        </>
      )}
      {footer && <div className="mt-1.5">{footer}</div>}
      {actions}
    </div>
  );
}

function TableShell({ children }: { children: ReactNode }) {
  return (
    <div className="overflow-x-auto rounded-md border border-white/8">
      <table className="w-full text-left text-xs">{children}</table>
    </div>
  );
}

function Th({
  children,
  align = 'left',
  tip,
}: {
  children?: ReactNode;
  align?: 'left' | 'right';
  tip?: HelpTip;
}) {
  return (
    <th className={cn('px-2 py-1.5', align === 'right' && 'text-right')}>
      {tip ? (
        <LabelTip tip={tip} className={align === 'right' ? 'justify-end' : undefined}>
          {children}
        </LabelTip>
      ) : (
        children
      )}
    </th>
  );
}

function Td({
  children,
  align = 'left',
  mono,
  tone,
}: {
  children?: ReactNode;
  align?: 'left' | 'right';
  mono?: boolean;
  tone?: 'good' | 'bad';
}) {
  return (
    <td
      className={cn(
        'px-2 py-1.5',
        align === 'right' && 'text-right',
        mono && 'font-mono',
        tone === 'bad' && 'text-red',
        tone === 'good' && 'text-text-mid',
      )}
    >
      {children}
    </td>
  );
}

// ── Formatting ──────────────────────────────────────────────────────────────

const fmt = (n: number | null | undefined, digits = 2): string =>
  n == null || !Number.isFinite(n) ? '—' : n.toFixed(digits);

const signed = (n: number, digits = 2): string => `${n >= 0 ? '+' : ''}${n.toFixed(digits)}`;

/** A percent that is already in percentage units (31.0 ⇒ `+31.0%`). */
const pctText = (n: number | null | undefined): string =>
  n == null || !Number.isFinite(n) ? '—' : `${signed(n, 1)}%`;

/** A 0..1 share (0.42 ⇒ `42%`). */
const pct01 = (n: number | null | undefined): string =>
  n == null || !Number.isFinite(n) ? '—' : `${(n * 100).toFixed(0)}%`;

/** A difference between two percents, which is percentage POINTS, not a percent. */
const ppText = (n: number): string => `${signed(n, 1)}pp`;

const TIPS = {
  draft: {
    title: 'Draft',
    body: 'The candidate the pooled fit ranked first, then replayed on the held-out target cohort. The big number is that replay — the level. Its fit number ranked it and means nothing as a level.',
  },
  ungated: {
    title: 'Ungated control',
    body: 'The same cohort with no entry gate at all: buy every matched token. It exists before any rule does, which is why it is a legitimate comparison where a saved rule is not. If the draft does not beat it, the entry side is not earning its place.',
  },
  capture: {
    title: 'Oracle capture',
    body: 'Realized PnL over the best exit that was actually available after each fill, priced through the same fill and cost models. Without it, a rule that takes 31 of 40 available points and one that takes 31 of 300 score identically. The "never had upside" count grades the ENTRY and needs no exit rule to exist.',
  },
  incumbent: {
    title: 'Incumbent',
    body: 'A saved rule scored on the same cohort, for comparison only. It supplies no threshold, no structure, no buy size and no cap — a search anchored to a promoted rule can only rediscover it.',
  },
  nMatched: {
    title: 'Matched tokens',
    body: 'Tokens the ENGINE matched against this fingerprint — every configured axis, first-slot axes included. Check it against a hand count: an ix-labels-only approximation of one reference cohort takes 3,440 where the engine takes 264.',
  },
  cohortUngated: {
    title: 'Ungated return',
    body: 'What this cohort pays with no gate at all. Cohort quality is separable from rule quality — one rule spans −13.8% to +40.8% across six siblings, so a rule is never reportable without the cohort it belongs to.',
  },
  pnlPct: {
    title: 'Return',
    body: 'Money over capital for the positions this term closed — Σ pnl ÷ Σ entry, never a mean of percents.',
  },
  gateRho: {
    title: 'rho vs axis',
    body: 'Spearman between the clause\'s admit rate and the varied axis value, across the family. At |rho| ≥ 0.80 the clause is refused: it re-reads the fingerprint axis rather than filtering within it. Both signs refuse — anti-selecting on the axis re-reads it just as much.',
  },
  fitCol: {
    title: 'Fit (rank only)',
    body: 'Pooled Σpnl ÷ Σentry across the fit cohorts. It produced the ordering and nothing else. On the reference family every candidate is negative here while the winner pays +31% on the held-out cohort — quoting this as a level is the one mistake the fit/validate split exists to prevent.',
  },
  targetCol: {
    title: 'Target return',
    body: 'The held-out cohort\'s return — the number to report. Row 1 carries the authority replay; the rest carry the fast archive fold.',
  },
  clearance: {
    title: 'Cost clearance',
    body: 'Median net oracle round trip over every priceable entry (losers included), against what one round trip costs at this buy size and the cohort\'s median pool depth. Measured on the ungated control, before any candidate exists — no exit rule beats the best exit, so a cohort under 1x here is refused or badged rather than searched. The band is derived from the run\'s own cost model, and cost is U-shaped in buy size, so it moves with the form.',
  },
  spread: {
    title: 'Fill spread',
    body: 'The same closes repriced at the friendliest honest fill (first-in-window + fee-only) against the run\'s own pricing. On one dump-scalp family this gap was 6.93pp/trade while the signal itself was near breakeven — execution was the entire loss. An edge smaller than its own spread is priced on fill luck, and live paper books the pessimistic side.',
  },
  level: {
    title: 'Asked → got',
    body: 'The threshold the term authored against the mean GROSS return it actually closed at. Gross on purpose: that is the quantity `m_position.pnl` reads, so the pair is comparable — the further gap down to the net return is execution, which the fill spread reports separately. Shown only where the units match; `stall >= 30` has no comparable realized number.',
  },
  delay: {
    title: 'Delay added',
    body: 'Seconds this clause adds to the mean time from token creation to the entry fill (the draft minus the draft without it). Positive means it holds entries back.',
  },
  captureDelta: {
    title: 'Capture delta',
    body: 'Points of oracle capture the clause costs. Negative means the entries it keeps took LESS of the money that was available than the entries it turned away — combined with a positive delay, that is a gate the move itself creates.',
  },
} satisfies Record<string, HelpTip>;
