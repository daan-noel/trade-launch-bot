/**
 * The Console's **Arms** section — the durable arm ledger over a date range.
 *
 * The Waiting lane above answers "what is armed right now" from the in-RAM
 * registry and loses a row the instant it disarms. This answers the question
 * that survives the tab: which tokens did each rule arm on over a window, how
 * long did it watch, and how did each episode end. See
 * `docs/plans/strategies/arm-ledger.md`.
 *
 * **Collapsed means no fetch.** The section mounts a server page plus a funnel
 * aggregate; rendering that body behind a closed header would pay for a read
 * nobody asked for on every Console visit. The body is unmounted while closed,
 * and an `a*` deep link forces it open before the first paint so a landing
 * cohort is never invisible.
 */

import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import { connectArmedChanged } from 'services/sse';
import { fetchArmsSummary } from 'services/api';
import { numericColKeys } from 'services/tableRequest';
import { DEFAULT_POSITIONS_QUERY } from 'hooks/useServerTable';
import { useGetStrategyRulesQuery } from 'store/sharedEndpoints';
import { useUiToggle } from 'hooks/useUiPrefs';
import type { TableQuery } from 'components/table/types';
import type { ArmFunnel } from 'lib/strategy/types';
import { hasArmCohortParams, useArmCohort } from '@live/pages/console/armCohort';
import { armPopulationKey, armSummaryBody } from '@live/pages/console/armRequest';
import { ArmFunnelStrip } from './ArmFunnelStrip';
import { ArmsFilterBar } from './ArmsFilterBar';
import { ArmsTable, armColumns } from './ArmsTable';

/** Coalesce a burst of arm/disarm frames into one refetch. Arms are far burstier
 *  than closes (a launch spike arms every matching rule at once), so this window
 *  is wider than History's — the ledger is a review surface, not a live lane. */
const COALESCE_MS = 2_000;

export const ConsoleArmsSection = memo(function ConsoleArmsSection() {
  const [params] = useSearchParams();
  const [open, setOpen] = useUiToggle('consoleArmsOpen', false);
  // A deep link into the arm cohort must not land behind a closed header. Runs
  // once per arrival with params, then leaves the toggle to the user.
  const deepLinked = hasArmCohortParams(params);
  useEffect(() => {
    if (deepLinked) setOpen(true);
  }, [deepLinked, setOpen]);

  return (
    <section id="console-arms" className="flex flex-col gap-2">
      <button
        type="button"
        className="text-left text-xs font-bold uppercase tracking-wider text-text-dim hover:text-text"
        onClick={() => setOpen((v) => !v)}
        title="Every (rule, token) arming episode the ledger holds — including the ones that never traded"
      >
        {open ? '▾' : '▸'} Arms
      </button>
      {open && <ArmsSectionBody />}
    </section>
  );
});

/** The fetching half. Mounted only while the section is open — see the module
 *  docstring: collapsing must stop the reads, not just hide their output. */
function ArmsSectionBody() {
  // Frozen per mount: a preset window's `from` bound must not slide on every
  // render, or the table refetches continuously.
  const [nowMs] = useState(() => Date.now());
  const cohort = useArmCohort(nowMs);
  const [reloadNonce, setReloadNonce] = useState(0);

  const { data: rules = [] } = useGetStrategyRulesQuery();
  const ruleNameOf = useCallback(
    (id: string | null) => (id ? (rules.find((r) => r.id === id)?.rule_name ?? null) : null),
    [rules],
  );

  // The table's own view state, owned here — this is what lets the funnel narrow
  // with the table instead of describing the unfiltered ledger.
  const [query, setQuery] = useState<TableQuery>(DEFAULT_POSITIONS_QUERY);
  const columns = useMemo(() => armColumns(ruleNameOf), [ruleNameOf]);
  const numericCols = useMemo(() => numericColKeys(columns), [columns]);

  const input = useMemo(() => ({ cohort, query, numericCols }), [cohort, query, numericCols]);
  // Page/sort deliberately excluded — paging the table must not re-run the
  // aggregate.
  const summaryKey = useMemo(
    () => `${armPopulationKey(input)}|${reloadNonce}`,
    [input, reloadNonce],
  );

  const [funnel, setFunnel] = useState<ArmFunnel | null>(null);
  const [funnelLoading, setFunnelLoading] = useState(true);

  useEffect(() => {
    const ctrl = new AbortController();
    setFunnelLoading(true);
    fetchArmsSummary(armSummaryBody(input), ctrl.signal)
      .then((f) => {
        if (ctrl.signal.aborted) return;
        setFunnel(f);
        setFunnelLoading(false);
      })
      .catch((e) => {
        if (ctrl.signal.aborted || (e instanceof DOMException && e.name === 'AbortError')) return;
        setFunnel(null);
        setFunnelLoading(false);
      });
    return () => ctrl.abort();
    // `input` changes identity per render; the population key is the real dep.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [summaryKey]);

  // Live arm/disarm frames invalidate the cohort — coalesced into one refetch of
  // the funnel and the table's current page.
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    const bump = () => {
      if (timer.current) return;
      timer.current = setTimeout(() => {
        timer.current = null;
        setReloadNonce((n) => n + 1);
      }, COALESCE_MS);
    };
    const h = connectArmedChanged(() => bump());
    return () => {
      h.close();
      if (timer.current) {
        clearTimeout(timer.current);
        timer.current = null;
      }
    };
  }, []);

  // Selection is section-local: an episode has no position id, so it cannot ride
  // the page's `position` deep-link param (which the position modals key on). The
  // table below opens the episode's own detail modal off this key.
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const onSelect = useCallback((key: string | null) => setSelectedKey(key), []);

  return (
    <>
      <ArmsFilterBar cohort={cohort} armedCount={funnelLoading ? null : (funnel?.armed ?? null)} />
      <ArmFunnelStrip funnel={funnel} loading={funnelLoading} cohort={cohort} />
      <ArmsTable
        cohort={cohort}
        columns={columns}
        numericCols={numericCols}
        query={query}
        onQueryChange={setQuery}
        ruleNameOf={ruleNameOf}
        selectedKey={selectedKey}
        onSelect={onSelect}
        reloadNonce={reloadNonce}
      />
    </>
  );
}
