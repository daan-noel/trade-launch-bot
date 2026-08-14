/**
 * The detail modal for one **arming episode** — the Arms table's row click, and
 * the ledger's answer to "the rule watched this token; why did nothing happen".
 *
 * It rides `FloorPositionDetail` for everything below the header: the chart, the
 * crosshair ↔ condition-band wiring, and the bar/range trades panel are the same
 * surfaces a position gets, and an episode is read the same way — scrub the chart,
 * watch the entry conditions approach (or not).
 *
 * The header is its own, though, through that component's `header` slot. An
 * episode has no fill: no entry SOL, no exit price, no PnL. The position hero
 * would render a `—%` in 3xl type over a strip of dashes, saying nothing. What an
 * episode *does* have is an outcome and a duration, so those take the same places.
 *
 * The chart carries **no markers** for the same reason a card doesn't (see
 * `liveChartCards`): a marker needs a price, and an arm has none.
 */

import { useMemo } from 'react';
import { Link } from 'react-router-dom';
import { DateCell } from 'components/table/DateCell';
import { ElapsedCell } from 'components/table/ElapsedCell';
import { ModeBadge } from 'components/strategy/ModeBadge';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { Modal } from 'components/ui/Modal';
import { useFlowPatternSourceForRule } from 'hooks/useFlowPatternKeys';
import { holdLabel } from 'lib/holdLabel';
import { STRATEGY_PATHS, ruleAnalyzeHref } from 'lib/strategy/nav';
import type { StrategyArmRecord } from 'lib/strategy/types';
import {
  FloorPositionDetail,
  InlineFact,
} from '@live/components/floor/FloorPositionDetail';
import { ArmedRuleConditions } from '@live/components/floor/LiveRuleConditions';
import { ARM_END_LABEL, armEndBadge } from '@live/components/floor/liveChartCards';
import { ArmEndDetailLine } from './armBlockers';

export function ArmDetailModal({
  arm,
  ruleName,
  onClose,
}: {
  arm: StrategyArmRecord;
  /** Resolved by the section (one rules cache for the whole page). */
  ruleName: string | null;
  onClose: () => void;
}) {
  const flow = useFlowPatternSourceForRule(arm.rule_id);
  // Memoized on the fields it reads: the modal re-renders on every clock tick of a
  // live episode, and a fresh object would rebuild the chart's (empty) marker set
  // each second.
  const inspect = useMemo(
    () => ({
      mint_address: arm.mint_address,
      entryTime: null,
      entryPrice: null,
      exitTime: null,
      exitPrice: null,
    }),
    [arm.mint_address],
  );

  const armedMs = Date.parse(arm.armed_at);
  const waited = holdLabel(arm.armed_at, arm.ended_at);
  // A live episode keeps ticking; an ended one is a fixed span (see `ElapsedCell`).
  const waitedNode = arm.ended_at ? (
    <span>{waited ?? '—'}</span>
  ) : (
    <ElapsedCell startMs={Number.isFinite(armedMs) ? armedMs : null} className="" />
  );

  const header = (
    <>
      <div className="rounded-lg border border-white/8 bg-bg-panel px-3 py-2.5">
        <div className="flex flex-wrap items-start gap-3">
          <div className="min-w-0 flex-1">
            <AddressDisplay
              address={arm.mint_address}
              kind="token"
              display={arm.symbol ?? undefined}
              reveal="always"
              actionsPlacement="right"
            />
            <div className="mt-1.5 inline-flex flex-wrap items-center gap-1">
              {armEndBadge(arm.end_reason)}
              {(arm.mode === 'paper' || arm.mode === 'real') && <ModeBadge mode={arm.mode} />}
            </div>
          </div>

          <div className="ml-auto text-right">
            <div className="text-3xl leading-none tabular-nums tracking-tight text-warning">
              {waitedNode}
            </div>
            <div className="mt-1 text-[10px] font-bold uppercase tracking-wider text-text-dim">
              Waited
            </div>
          </div>
        </div>

        {/* The disarm's recorded reason, above the fold. The condition strip
            below reconstructs the same instant from stored trades, but that is
            retention-bound and reads the rule's CURRENT params — this is what
            the fold actually saw, and it is the first thing to look at. */}
        <ArmEndDetailLine detail={arm.end_detail} />

        <div className="mt-2 flex flex-wrap items-center gap-3 text-[11px]">
          <Link
            to={ruleAnalyzeHref(arm.rule_id)}
            className="font-semibold text-accent hover:underline"
            onClick={(e) => e.stopPropagation()}
          >
            Evidence · {ruleName ?? arm.rule_id.slice(0, 8)}
          </Link>
          <Link
            to={`${STRATEGY_PATHS.console}?mint=${encodeURIComponent(arm.mint_address)}`}
            className="font-semibold text-accent hover:underline"
            onClick={(e) => e.stopPropagation()}
          >
            Trade
          </Link>
        </div>
      </div>

      <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1.5 rounded-lg border border-white/8 bg-bg-panel/80 px-3 py-2 text-[12px]">
        <InlineFact label="Armed" value={<DateCell iso={arm.armed_at} />} />
        <InlineFact
          label="Ended"
          value={arm.ended_at ? <DateCell iso={arm.ended_at} /> : '—'}
          muted={!arm.ended_at}
        />
        <InlineFact label="Waited" value={waitedNode} />
        <InlineFact
          label="Outcome"
          value={arm.end_reason ? (ARM_END_LABEL[arm.end_reason] ?? arm.end_reason) : 'waiting'}
        />
        {/* The position id is shown, not linked: the closed-position modal belongs
            to the History section and only opens a row on ITS current page, so a
            link here would silently do nothing for most episodes. */}
        {arm.position_id ? (
          <InlineFact
            label="Position"
            value={<span className="font-mono">{arm.position_id.slice(0, 8)}…</span>}
            muted
          />
        ) : null}
      </div>
    </>
  );

  return (
    <Modal
      title={
        <span className="inline-flex flex-wrap items-center gap-2">
          <span className={arm.symbol ? undefined : 'font-mono'}>
            {arm.symbol || `${arm.mint_address.slice(0, 8)}…`}
          </span>
          {armEndBadge(arm.end_reason)}
          <span className="text-[11px] font-normal tabular-nums text-text-dim">
            waited {waitedNode}
          </span>
        </span>
      }
      open
      onClose={onClose}
      size="xxl"
    >
      <FloorPositionDetail
        chartHeight={360}
        header={header}
        facts={{
          mint: arm.mint_address,
          symbol: arm.symbol,
          ruleId: arm.rule_id,
          ruleName,
          mode: arm.mode,
          // Unused by the custom header, but the shape requires a status and a
          // reader of `facts` should not see a lie.
          status: arm.end_reason ? (ARM_END_LABEL[arm.end_reason] ?? arm.end_reason) : 'Waiting',
          inspect,
          flowPatternKeys: flow.keys,
          flowFingerprintId: flow.fingerprintId,
          // The armed side's readout: which entry conditions failed, and how close
          // they came, is exactly why this episode ended the way it did. Pinned at
          // the disarm instant for an ended row; live for one still waiting.
          conditions: (
            <ArmedRuleConditions
              mint={arm.mint_address}
              ruleId={arm.rule_id}
              armedAt={arm.armed_at}
              endedAt={arm.ended_at}
            />
          ),
        }}
      />
    </Modal>
  );
}
