import { useMemo } from 'react';

import {
  ToggleGroup,
  type ToggleGroupOption,
  type ToggleGroupSize,
} from 'components/ui/ToggleGroup';
import {
  MODE_TOGGLE_ACTIVE,
  MODE_TOGGLE_SWATCH,
  type ModeFilter,
  type ModeToggleLayout,
} from 'lib/strategy/mode';
import type { TradeMode } from 'lib/strategy/types';

type ModeCounts = Partial<Record<TradeMode, number>>;

type ModeToggleBase = {
  /** Optional per-mode counts (rule boards). Omit on ops surfaces. */
  counts?: ModeCounts;
  layout?: ModeToggleLayout;
  size?: ToggleGroupSize;
  className?: string;
  'aria-label'?: string;
};

export type ModeToggleProps =
  | (ModeToggleBase & {
      /** Include the All segment (default). */
      includeAll?: true;
      value: ModeFilter;
      onChange: (next: ModeFilter) => void;
    })
  | (ModeToggleBase & {
      /** Real · Paper only (Portfolio). */
      includeAll: false;
      value: TradeMode;
      onChange: (next: TradeMode) => void;
    });

function modeLabel(mode: TradeMode): string {
  return mode === 'real' ? 'REAL' : 'PAPER';
}

function buildOptions(
  includeAll: boolean,
  layout: ModeToggleLayout,
  counts: ModeCounts | undefined,
): ToggleGroupOption<ModeFilter>[] {
  const paperCount = counts?.paper;
  const realCount = counts?.real;
  const hasCounts = paperCount != null && realCount != null;

  const paper: ToggleGroupOption<ModeFilter> = {
    value: 'paper',
    label: modeLabel('paper'),
    swatch: MODE_TOGGLE_SWATCH.paper,
    activeClassName: MODE_TOGGLE_ACTIVE.paper,
    count: paperCount,
    title:
      paperCount != null
        ? `Show only the ${paperCount} paper rule(s)`
        : 'Show paper only',
  };
  const real: ToggleGroupOption<ModeFilter> = {
    value: 'real',
    label: modeLabel('real'),
    swatch: MODE_TOGGLE_SWATCH.real,
    activeClassName: MODE_TOGGLE_ACTIVE.real,
    count: realCount,
    title:
      realCount != null
        ? `Show only the ${realCount} real-money rule(s)`
        : 'Show real only',
  };
  const all: ToggleGroupOption<ModeFilter> = {
    value: 'all',
    label: 'All',
    activeClassName: MODE_TOGGLE_ACTIVE.all,
    count: hasCounts ? paperCount + realCount : undefined,
    title: hasCounts
      ? `Show both modes (${paperCount} paper, ${realCount} real)`
      : 'Show both modes',
  };

  if (!includeAll) return layout === 'ops' ? [real, paper] : [paper, real];
  return layout === 'ops' ? [real, paper, all] : [all, paper, real];
}

/**
 * Shared All / Paper / Real segmented control — one chrome for every mode filter.
 * Rule boards use `RuleModeFilter` (counts); ops pages use this directly.
 */
export function ModeToggle(props: ModeToggleProps) {
  const {
    value,
    onChange,
    counts,
    layout = 'filter',
    size = 'md',
    className,
    'aria-label': ariaLabel = 'Trade mode',
  } = props;
  const includeAll = props.includeAll !== false;

  const options = useMemo(
    () => buildOptions(includeAll, layout, counts),
    [includeAll, layout, counts],
  );

  return (
    <ToggleGroup
      aria-label={ariaLabel}
      tone="mode"
      size={size}
      options={options}
      value={value}
      onChange={onChange as (next: ModeFilter) => void}
      className={className}
    />
  );
}
