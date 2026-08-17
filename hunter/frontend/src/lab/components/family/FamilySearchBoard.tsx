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
  FamilyThresholdLadder,
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

      {report.standing_terms.length > 0 && (
        <div className="rounded-lg border border-dashed border-white/12 bg-white/2 p-3">
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
              Standing exit
            </span>
            {report.standing_terms.map((t) => (
              <Badge key={t} variant="neutral" size="sm">
                {t}
              </Badge>
            ))}
          </div>
          <p className="mt-1 text-[11px] leading-snug text-text-dim">
            Carried by every rule scored here, the ungated control included — mechanics you
            asked for, so the numbers describe a rule you would actually run. Never searched,
            never ablated, and never credited with the edge.
          </p>
        </div>
      )}

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
            emptyHint={
              report.selection?.none_cleared
                ? `Nothing cleared both bars — ${report.selection.n_rejected} tried. The entry side had to win more than ${report.selection.win_bar_pct.toFixed(0)}% of its closes and the exit side had to make money.`
                : 'No candidate survived the fit.'
            }
            stats={
              draft
                ? [
                    {
                      label: report.selection?.win_within_noise
                        ? 'win rate (inside noise)'
                        : 'win rate',
                      value:
                        draft.target_win_pct == null
                          ? '—'
                          : `${draft.target_win_pct.toFixed(0)}% of ${draft.target_n_closed}${
                              report.selection?.draft_win_low_pct == null
                                ? ''
                                : ` (≥${report.selection.draft_win_low_pct.toFixed(0)}%)`
                            }`,
                    },
                    { label: 'PnL', value: `${fmt(draft.target_pnl_sol, 3)} ◎` },
                    { label: 'entered', value: `${pct01(draft.target_enter_pct)} of matched` },
                    {
                      label: 'shape',
                      value: `${draft.n_entry_quantities} entry · ${draft.n_alarms} alarms`,
                    },
                  ]
                : []
            }
            footer={
              draft ? (
                <>
                  <p className="text-[10px] leading-tight text-text-dim">
                    fit {pctText(draft.fit_ret_pct)}{' '}
                    <span className="text-text-dim/70">· rank only, never a level</span>
                  </p>
                  {report.selection?.win_within_noise && (
                    <p className="mt-1 text-[11px] leading-snug text-warning">
                      the win rate clears the {report.selection.win_bar_pct.toFixed(0)}% bar as a
                      point estimate, but its lower bound does not — the safety edge is inside
                      this sample&apos;s noise
                    </p>
                  )}
                </>
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
                    {
                      label: 'win rate (the bar)',
                      value:
                        ungated.target_win_pct == null
                          ? '—'
                          : `${ungated.target_win_pct.toFixed(0)}%`,
                    },
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
              {
                label: 'never did',
                value:
                  capture.no_upside_pct == null
                    ? String(capture.n_no_upside)
                    : `${capture.n_no_upside} · ${capture.no_upside_pct.toFixed(0)}%`,
              },
              {
                label: 'took / available',
                value: `${fmt(capture.realized_pnl_sol, 3)} / ${fmt(capture.oracle_pnl_sol, 3)} ◎`,
              },
            ]}
            footer={<EntryFilterLine report={report} />}
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
              <Th align="right" tip={TIPS.cohortUngatedWin}>
                Ungated win
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
                <Td align="right" mono>
                  {m.ungated_win_pct == null ? '—' : `${m.ungated_win_pct.toFixed(0)}%`}
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
                <Th align="right" tip={TIPS.alarmWin}>
                  Win rate
                </Th>
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
                      {a.standing && (
                        <span className="ml-1.5 align-middle">
                          <Badge
                            variant="neutral"
                            size="sm"
                            title="A mechanical exit you asked for, not a discovered alarm — never credited with the edge"
                          >
                            standing
                          </Badge>
                        </span>
                      )}
                    </Td>
                    <Td align="right" mono>
                      {a.n}
                    </Td>
                    <Td align="right" mono>
                      {a.win_rate_pct == null
                        ? '—'
                        : `${a.win_rate_pct.toFixed(0)}% (${a.n_wins})`}
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

      {/* ── Was firing right? ────────────────────────────────────────────── */}
      {report.alarm_regret.length > 0 && (
        <Section
          title="Was each alarm right to fire?"
          caption="The table above says which alarm made the money; this one says whether it fired at the right moment. Every close is graded against two counterfactuals it costs nothing to compute: the best exit still available afterwards, and simply holding to the token's last print."
        >
          <TableShell>
            <thead className="text-[10px] uppercase tracking-wide text-text-dim">
              <tr>
                <Th>Exit term</Th>
                <Th align="right">Graded</Th>
                <Th align="right" tip={TIPS.forfeit}>
                  Left on the table
                </Th>
                <Th align="right" tip={TIPS.vsHold}>
                  vs holding on
                </Th>
                <Th>Verdict</Th>
              </tr>
            </thead>
            <tbody>
              {report.alarm_regret.map((a) => (
                <tr key={a.slot} className="border-t border-white/6">
                  <Td>
                    <span className="font-mono text-text-mid">
                      {a.label ?? `slot ${a.slot} (unnamed)`}
                    </span>
                    {a.standing && (
                      <span className="ml-1.5 align-middle">
                        <Badge
                          variant="neutral"
                          size="sm"
                          title="A mechanical exit you asked for — reported, never a finding"
                        >
                          standing
                        </Badge>
                      </span>
                    )}
                  </Td>
                  <Td align="right" mono>
                    <span title={`${a.n} closed, ${a.n_priced} with a print after them to grade against`}>
                      {a.n_priced} / {a.n}
                    </span>
                  </Td>
                  <Td
                    align="right"
                    mono
                    tone={a.forfeit_pp > a.band_pct ? 'bad' : 'good'}
                  >
                    {a.n_priced === 0 ? '—' : ppText(a.forfeit_pp)}
                  </Td>
                  <Td
                    align="right"
                    mono
                    tone={a.realized_vs_terminal_pp < 0 ? 'bad' : 'good'}
                  >
                    {a.n_terminal === 0 ? '—' : ppText(a.realized_vs_terminal_pp)}
                  </Td>
                  <Td>
                    <RegretVerdictCell verdict={a.verdict} band={a.band_pct} />
                  </Td>
                </tr>
              ))}
            </tbody>
          </TableShell>
          <p className="mt-2 text-[11px] leading-snug text-text-dim">
            &ldquo;Left on the table&rdquo; under one round trip is not forfeited money — it could
            not have been collected. An alarm that leaves real upside behind is still doing its
            job while holding on would have paid less; only when BOTH go the wrong way is it
            cutting winners.
          </p>
        </Section>
      )}

      {/* ── Narrow re-check ──────────────────────────────────────────────── */}
      {report.narrow_recheck.length > 0 && (
        <Section
          title="What each condition is worth here"
          caption="The draft re-scored on the target cohort with one condition dropped. Each side is graded in its own currency: an ENTRY condition earns its place by raising the win rate, an EXIT alarm by raising the return. Grading both on money alone is what deletes every entry condition."
        >
          <TableShell>
            <thead className="text-[10px] uppercase tracking-wide text-text-dim">
              <tr>
                <Th>Dropped condition</Th>
                <Th>Side</Th>
                <Th align="right" tip={TIPS.winDelta}>
                  Win rate Δ
                </Th>
                <Th align="right">Return Δ</Th>
                <Th align="right">With → without</Th>
                <Th />
              </tr>
            </thead>
            <tbody>
              {report.narrow_recheck.map((t) => (
                <tr key={`${t.is_entry}-${t.label}`} className="border-t border-white/6">
                  <Td>
                    <span className="font-mono text-text-mid">{t.label}</span>
                  </Td>
                  <Td>
                    <Badge variant={t.is_entry ? 'primary' : 'info'} size="sm">
                      {t.is_entry ? 'entry · safety' : 'exit · profit'}
                    </Badge>
                  </Td>
                  <Td
                    align="right"
                    mono
                    tone={
                      t.win_delta_pp == null ? undefined : t.win_delta_pp < 0 ? 'bad' : 'good'
                    }
                  >
                    {t.win_delta_pp == null ? '—' : ppText(t.win_delta_pp)}
                  </Td>
                  <Td align="right" mono tone={t.delta_pct < 0 ? 'bad' : 'good'}>
                    {ppText(t.delta_pct)}
                  </Td>
                  <Td align="right" mono>
                    <span className="text-text-dim">
                      {pctText(t.ret_full_pct)} → {pctText(t.ret_without_pct)}
                    </span>
                  </Td>
                  <Td>
                    {t.inert ? (
                      <Badge
                        variant="neutral"
                        size="sm"
                        title="Dropping it changed nothing at all on this cohort — dead weight here"
                      >
                        inert
                      </Badge>
                    ) : t.earns_its_place ? (
                      <Badge variant="success" size="sm" title="It pays in its own side's currency">
                        earns it
                      </Badge>
                    ) : (
                      <Badge variant="warning" size="sm" title="It costs more than it buys">
                        costs
                      </Badge>
                    )}
                  </Td>
                </tr>
              ))}
            </tbody>
          </TableShell>
          <p className="mt-2 text-[11px] leading-snug text-text-dim">
            Δ is the draft minus the version without that condition, so positive means the
            condition earns its place. An entry condition that costs a point of return while
            buying ten of win rate is doing exactly its job.
          </p>
        </Section>
      )}

      {/* ── Threshold ladders ────────────────────────────────────────────── */}
      {report.threshold_ladders.length > 0 && (
        <Section
          title="Is each threshold a level, or a lucky number?"
          caption="The draft replayed at neighbouring cuts, one clause at a time. A cut whose neighbours score alike is a REGION the launch shape defines; one that collapses a step either way is a value fitted to this cohort's noise. Each side is read in its own currency — win rate for an entry clause, return for an exit alarm."
          right={
            <span className="text-[11px] text-text-dim">
              {report.threshold_ladders.filter((l) => l.verdict === 'fragile').length} of{' '}
              {report.threshold_ladders.length} on a spike
            </span>
          }
        >
          <div className="space-y-2">
            {report.threshold_ladders.map((l) => (
              <LadderRow key={`${l.is_entry}-${l.clause}`} ladder={l} />
            ))}
          </div>
          <p className="mt-2 text-[11px] leading-snug text-text-dim">
            The bar under each threshold is that cut&apos;s grade against the best on the ladder;
            the outlined one is the draft&apos;s own value. This is the check that catches the
            recorded failure mode — the entry band being the whole cost of a rule — which a
            single-point test cannot see.
          </p>
        </Section>
      )}

      {/* ── Fill sensitivity, per clause ─────────────────────────────────── */}
      {report.fill_sensitivity.length > 0 && (
        <Section
          title="Which conditions survive the fill"
          caption="Each condition's contribution measured twice: once at this run's pricing, once at the friendliest honest fill. The whole-rule spread above asks whether the RESULT is real; this asks the same of every clause that produced it — a contribution that flips or collapses between the two is selecting fill luck."
        >
          <TableShell>
            <thead className="text-[10px] uppercase tracking-wide text-text-dim">
              <tr>
                <Th>Condition</Th>
                <Th>Side</Th>
                <Th align="right" tip={TIPS.deltaAuthority}>
                  Worth (this fill)
                </Th>
                <Th align="right">Worth (optimistic)</Th>
                <Th>Verdict</Th>
              </tr>
            </thead>
            <tbody>
              {report.fill_sensitivity.map((f) => (
                <tr
                  key={`${f.is_entry}-${f.clause}`}
                  className={cn('border-t border-white/6', f.fill_dependent && 'bg-warning/5')}
                >
                  <Td>
                    <span className="font-mono text-text-mid">{f.clause}</span>
                  </Td>
                  <Td>
                    <Badge variant={f.is_entry ? 'primary' : 'info'} size="sm">
                      {f.is_entry ? 'entry · win rate' : 'exit · return'}
                    </Badge>
                  </Td>
                  <Td
                    align="right"
                    mono
                    tone={
                      f.delta_authority == null
                        ? undefined
                        : f.delta_authority < 0
                          ? 'bad'
                          : 'good'
                    }
                  >
                    {f.delta_authority == null ? '—' : ppText(f.delta_authority)}
                  </Td>
                  <Td
                    align="right"
                    mono
                    tone={
                      f.delta_optimistic == null
                        ? undefined
                        : f.delta_optimistic < 0
                          ? 'bad'
                          : 'good'
                    }
                  >
                    {f.delta_optimistic == null ? '—' : ppText(f.delta_optimistic)}
                  </Td>
                  <Td>
                    {f.fill_dependent ? (
                      <Badge
                        variant="warning"
                        size="sm"
                        title="The contribution flips sign or keeps under a quarter of itself between the two pricings"
                      >
                        fill-shaped
                      </Badge>
                    ) : (
                      <span className="text-[11px] text-text-dim">holds across pricings</span>
                    )}
                  </Td>
                </tr>
              ))}
            </tbody>
          </TableShell>
        </Section>
      )}

      {/* ── Entry redundancy ─────────────────────────────────────────────── */}
      {report.entry_redundancy.length > 0 && (
        <Section
          title="Does each entry condition filter anything of its own?"
          caption="Dropping one condition is a first-order test: two conditions that turn away the same tokens each look near-worthless alone, because the other covers for it. This adds the two views that see it — the condition on its own, and how much of what it vetoes a sibling already vetoes."
        >
          <TableShell>
            <thead className="text-[10px] uppercase tracking-wide text-text-dim">
              <tr>
                <Th>Entry condition</Th>
                <Th align="right" tip={TIPS.vetoed}>
                  Turns away
                </Th>
                <Th align="right" tip={TIPS.solo}>
                  On its own
                </Th>
                <Th align="right" tip={TIPS.overlap}>
                  Overlap
                </Th>
                <Th>Verdict</Th>
              </tr>
            </thead>
            <tbody>
              {report.entry_redundancy.map((x) => (
                <tr
                  key={x.clause}
                  className={cn('border-t border-white/6', x.redundant && 'bg-warning/5')}
                >
                  <Td>
                    <span className="font-mono text-text-mid">{x.clause}</span>
                  </Td>
                  <Td align="right" mono>
                    {x.n_vetoed}
                  </Td>
                  <Td align="right" mono>
                    <span
                      title={`Alone with the full exit bag: ${pctText(x.solo_ret_pct)} over ${x.solo_n_closed} closes`}
                    >
                      {x.solo_win_pct == null ? '—' : `${x.solo_win_pct.toFixed(0)}%`}
                      <span className="ml-1 text-text-dim">{pctText(x.solo_ret_pct)}</span>
                    </span>
                  </Td>
                  <Td align="right" mono>
                    {x.max_overlap_pct == null ? (
                      '—'
                    ) : (
                      <span title={x.overlap_with ? `with \`${x.overlap_with}\`` : undefined}>
                        {x.max_overlap_pct.toFixed(0)}%
                      </span>
                    )}
                  </Td>
                  <Td>
                    {x.redundant ? (
                      <span className="inline-flex flex-col gap-0.5">
                        <Badge variant="warning" size="sm">
                          covered for
                        </Badge>
                        {x.overlap_with && (
                          <span className="text-[11px] leading-snug text-text-dim">
                            {x.max_overlap_pct?.toFixed(0)}% of its vetoes are also{' '}
                            <code className="text-text-mid">{x.overlap_with}</code>&apos;s
                          </span>
                        )}
                      </span>
                    ) : (
                      <span className="text-[11px] text-text-dim">
                        {x.n_vetoed === 0 ? 'turns nothing away here' : 'filters on its own'}
                      </span>
                    )}
                  </Td>
                </tr>
              ))}
            </tbody>
          </TableShell>
        </Section>
      )}

      {/* ── Enrich ───────────────────────────────────────────────────────── */}
      {report.enrich.length > 0 && (
        <Section
          title="Conditions offered to the draft"
          caption="Every other condition this cohort earns, tried one at a time on top of the fitted rule — the only stage that can make a rule denser. An entry idea has to raise the win rate, an exit alarm has to raise the return, and each accepted one is re-checked against the rule as it grows so two forms of the same idea cannot both get in."
          right={
            <span className="text-[11px] text-text-dim">
              {report.enrich.filter((e) => e.accepted).length} of {report.enrich.length} accepted
            </span>
          }
        >
          <TableShell>
            <thead className="text-[10px] uppercase tracking-wide text-text-dim">
              <tr>
                <Th>Condition</Th>
                <Th>Side</Th>
                <Th align="right">Win rate Δ</Th>
                <Th align="right">Return Δ</Th>
                <Th align="right">Closes after</Th>
                <Th>Verdict</Th>
              </tr>
            </thead>
            <tbody>
              {report.enrich.map((e) => (
                <tr
                  key={`${e.is_entry}-${e.label}`}
                  className={cn('border-t border-white/6', e.accepted && 'bg-green/5')}
                >
                  <Td>
                    <span className="font-mono text-text-mid">{e.label}</span>
                  </Td>
                  <Td>
                    <Badge variant={e.is_entry ? 'primary' : 'info'} size="sm">
                      {e.is_entry ? 'entry · safety' : 'exit · profit'}
                    </Badge>
                  </Td>
                  <Td
                    align="right"
                    mono
                    tone={e.win_delta_pp == null ? undefined : e.win_delta_pp < 0 ? 'bad' : 'good'}
                  >
                    {e.win_delta_pp == null ? '—' : ppText(e.win_delta_pp)}
                  </Td>
                  <Td align="right" mono tone={e.ret_delta_pct < 0 ? 'bad' : 'good'}>
                    {ppText(e.ret_delta_pct)}
                  </Td>
                  <Td align="right" mono>
                    {e.n_closed_after}
                  </Td>
                  <Td>
                    {e.accepted ? (
                      <Badge variant="success" size="sm">
                        added
                      </Badge>
                    ) : (
                      <span className="text-[11px] text-text-dim">{e.refused ?? 'refused'}</span>
                    )}
                  </Td>
                </tr>
              ))}
            </tbody>
          </TableShell>
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
                <Th align="right" tip={TIPS.winCol}>
                  Win rate
                </Th>
                <Th align="right" tip={TIPS.shapeCol}>
                  Shape
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
                    {row.target_win_pct == null ? '—' : `${row.target_win_pct.toFixed(0)}%`}
                  </Td>
                  <Td align="right" mono>
                    <span className="text-text-dim">
                      {row.n_entry_quantities}e · {row.n_alarms}x
                    </span>
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

/**
 * One clause's threshold ladder as a bar strip: each cut's grade against the best on
 * the ladder, the draft's own value outlined. The shape is the finding — a flat strip
 * is a region the launch shape defines, a lone tall bar is a value fitted to noise.
 *
 * Bars are drawn from the ladder's own min so a strip of similar grades reads as
 * similar. Scaling from zero would flatten every real difference on a cohort whose
 * grades all sit near 30%, which is exactly the case this chart exists to resolve.
 */
function LadderRow({ ladder }: { ladder: FamilyThresholdLadder }) {
  const grade = (p: FamilyThresholdLadder['points'][number]): number | null =>
    ladder.is_entry ? p.win_pct : p.ret_pct;
  const graded = ladder.points.filter((p) => grade(p) != null);
  const values = graded.map((p) => grade(p) as number);
  const min = Math.min(...values);
  const max = Math.max(...values);
  const span = max - min;
  const height = (v: number): string =>
    span <= 0 ? '55%' : `${12 + 88 * ((v - min) / span)}%`;

  const tone =
    ladder.verdict === 'fragile'
      ? 'danger'
      : ladder.verdict === 'plateau'
        ? 'success'
        : 'neutral';

  return (
    <div className="rounded-md border border-white/8 bg-white/2 p-2.5">
      <div className="mb-1.5 flex flex-wrap items-center gap-2">
        <span className="font-mono text-xs text-text-mid">{ladder.clause}</span>
        <Badge variant={ladder.is_entry ? 'primary' : 'info'} size="sm">
          {ladder.is_entry ? 'entry · win rate' : 'exit · return'}
        </Badge>
        <Badge variant={tone} size="sm" title={LADDER_VERDICTS[ladder.verdict]}>
          {ladder.verdict}
        </Badge>
        <span className="text-[11px] text-text-dim">{LADDER_VERDICTS[ladder.verdict]}</span>
      </div>
      {graded.length === 0 ? (
        <p className="text-[11px] text-text-dim">
          No neighbouring cut produced a comparable grade on this cohort.
        </p>
      ) : (
        <div className="flex items-end gap-1.5" style={{ height: '3.25rem' }}>
          {graded.map((p) => {
            const g = grade(p) as number;
            return (
              <div
                key={p.threshold}
                className="flex min-w-0 flex-1 flex-col items-center justify-end gap-0.5"
                title={`${fmt(p.threshold, 3)} → ${
                  ladder.is_entry
                    ? `${g.toFixed(1)}% win rate`
                    : `${pctText(g)} return`
                } over ${p.n_closed} closes`}
              >
                <span
                  className={cn(
                    'w-full rounded-sm',
                    p.chosen
                      ? 'bg-primary/70 ring-1 ring-primary'
                      : g === max
                        ? 'bg-green/40'
                        : 'bg-white/15',
                  )}
                  style={{ height: height(g) }}
                />
                <span
                  className={cn(
                    'truncate text-[9px] leading-none',
                    p.chosen ? 'text-text-mid' : 'text-text-dim/70',
                  )}
                >
                  {fmt(p.threshold, 2)}
                </span>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

const LADDER_VERDICTS: Record<string, string> = {
  plateau: 'neighbouring cuts score alike — the level is a region, not a lucky value',
  fragile: 'one step either way gives back more than half the range — tuned to this cohort',
  flat: 'the cohort barely responds to this threshold at all',
  sparse: 'too few comparable cuts to judge',
};

/** An alarm's timing verdict, with the sentence that makes it actionable. */
function RegretVerdictCell({ verdict, band }: { verdict: string; band: number }) {
  if (verdict === 'premature') {
    return (
      <span className="inline-flex flex-col gap-0.5">
        <Badge variant="warning" size="sm">
          cuts winners
        </Badge>
        <span className="text-[11px] leading-snug text-text-dim">
          it left real upside AND holding on would have paid more
        </span>
      </span>
    );
  }
  if (verdict === 'protective') {
    return (
      <span className="inline-flex flex-col gap-0.5">
        <Badge variant="success" size="sm">
          early, and right
        </Badge>
        <span className="text-[11px] leading-snug text-text-dim">
          it left money, but holding on would have lost more
        </span>
      </span>
    );
  }
  if (verdict === 'timed') {
    return (
      <span className="inline-flex flex-col gap-0.5">
        <Badge variant="success" size="sm">
          timed
        </Badge>
        <span className="text-[11px] leading-snug text-text-dim">
          nothing beyond the {band.toFixed(1)}% round trip was left after it
        </span>
      </span>
    );
  }
  return <span className="text-[11px] text-text-dim">no print after the close to grade against</span>;
}

/**
 * Did the entry conditions reject losers, or just trade less? The oracle answers it
 * with no exit rule involved: the share of entries that never had a profitable exit,
 * against the same share for buying everything.
 */
function EntryFilterLine({ report }: { report: FamilySearchReport }) {
  const mine = report.capture.no_upside_pct;
  const theirs = report.ungated_capture?.no_upside_pct ?? null;
  if (mine == null || theirs == null) {
    return (
      <p className="text-[10px] leading-tight text-text-dim">
        the second count grades the <span className="text-text-mid">entry</span> — those tokens
        had nothing for any exit to take
      </p>
    );
  }
  const better = mine + 1 < theirs;
  return (
    <p className={cn('text-[11px] leading-snug', better ? 'text-green' : 'text-warning')}>
      {mine.toFixed(0)}% of its buys had no upside at all, against {theirs.toFixed(0)}% for
      buying everything —{' '}
      {better ? 'the entry rejects real losers' : 'the entry trades less without picking better'}
    </p>
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
  alarmWin: {
    title: 'Win rate',
    body: 'Of the positions this alarm closed, the share that closed up. Money alone cannot separate "fires often and wins rarely" from "fires rarely and wins" — one large win hides a hundred small losses in a sum.',
  },
  winDelta: {
    title: 'Win rate Δ',
    body: 'Points of win rate the condition is worth: the draft minus the draft without it. This is the currency of the ENTRY side — an entry condition that costs a point of return while buying ten of win rate is doing exactly its job, and grading it on money is what makes a search delete every entry condition.',
  },
  winCol: {
    title: 'Win rate',
    body: 'Share of closed positions that closed up, on the held-out cohort. Entry decides safety and exit decides profit, so the draft has to clear a win-rate bar as well as make money — the bar is whatever buying everything achieves on this cohort.',
  },
  shapeCol: {
    title: 'Shape',
    body: 'Entry IDEAS and exit alarms. A band (floor + ceiling on one quantity) is one idea written as two clauses, which is why a rule showing five entry metrics carries three. Alarms count only searched ones — a standing term is a mechanic, not an alarm.',
  },
  cohortUngatedWin: {
    title: 'Ungated win',
    body: 'What share of trades buying everything wins on this cohort. On the target cohort this is the bar the entry side of the draft must beat: a gate that does not enter more safely than buying everything is not filtering anything.',
  },
  captureDelta: {
    title: 'Capture delta',
    body: 'Points of oracle capture the clause costs. Negative means the entries it keeps took LESS of the money that was available than the entries it turned away — combined with a positive delay, that is a gate the move itself creates.',
  },
  forfeit: {
    title: 'Left on the table',
    body: 'Points the best exit still available AFTER each close would have paid over what the alarm took, pooled by money across the closes that had a print to grade against. Under one round trip it is not forfeited money — it could not have been collected at this buy size.',
  },
  vsHold: {
    title: 'vs holding on',
    body: 'The same closes against simply riding to the token\'s last print. Positive means firing beat holding. On this token class the last print is routinely near zero, which is why an alarm that leaves upside behind can still be exactly right — only when this AND the forfeit both go the wrong way is it cutting winners.',
  },
  deltaAuthority: {
    title: 'Worth (this fill)',
    body: 'The condition\'s drop-one contribution in its own side\'s currency — win-rate points for an entry condition, return points for an exit alarm — at this run\'s pricing. The next column is the same measurement at first-in-window fills with fee-only costs.',
  },
  vetoed: {
    title: 'Turns away',
    body: 'Tokens the draft would have entered without this condition and does not with it — the condition\'s own veto set, read off the drop-one replay at no extra cost.',
  },
  solo: {
    title: 'On its own',
    body: 'The condition alone as the entire entry gate, with the draft\'s full exit bag. Together with the full rule and the drop-one variant it triangulates the three cases ablation alone cannot separate: synergy, redundancy, and dead weight.',
  },
  overlap: {
    title: 'Overlap',
    body: 'The largest share of this condition\'s vetoes that another entry condition also vetoes. At 90% or more it is flagged: its filtering is a subset of a sibling\'s, so drop-one ablation shows a small delta only because the sibling covers for it.',
  },
} satisfies Record<string, HelpTip>;
