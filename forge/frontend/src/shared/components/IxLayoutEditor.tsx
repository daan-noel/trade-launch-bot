import { useMemo } from 'react';
import clsx from 'clsx';
import { Badge, Button, Field } from './ui';
import type { DecoStep } from '@shared/types';

// Mirrors `executor_core::IxLayout` — the shape (decoration step order) around the
// opaque on-chain `Core`. This editor authors a `DecoStep[]`; the backend
// `IxLayout::validate` is the fail-closed source of truth, and `validateLayout`
// below mirrors its rails so the operator sees a problem before saving.

export const ALL_STEPS: DecoStep[] = ['cu_limit', 'cu_price', 'create_ata', 'core', 'tip'];

const STEP_LABEL: Record<DecoStep, string> = {
  cu_limit: 'CU limit',
  cu_price: 'CU price',
  create_ata: 'Create ATA',
  core: 'Core',
  tip: 'Tip',
};

export type LayoutKind = 'buy' | 'create';

/** Client-side mirror of `IxLayout::validate`. Returns an error string, or null. */
export function validateLayout(steps: DecoStep[], kind: LayoutKind, isSnipe: boolean): string | null {
  const coreCount = steps.filter((s) => s === 'core').length;
  if (coreCount !== 1) return 'layout must contain exactly one Core step';
  const corePos = steps.indexOf('core');
  const ataPos = steps.indexOf('create_ata');
  if (ataPos !== -1) {
    if (kind === 'create') return 'create layout must not carry a standalone Create ATA (folded into Core)';
    if (ataPos > corePos) return 'Create ATA must precede Core';
  }
  if (isSnipe) {
    if (!steps.includes('cu_price')) return 'a snipe leg must include CU price';
    if (!steps.includes('tip')) return 'a snipe leg must include Tip';
  }
  return null;
}

/** The canonical default shape for a kind — matches `canonical_buy` / `canonical_create`. */
export function canonicalSteps(kind: LayoutKind): DecoStep[] {
  return kind === 'create'
    ? ['cu_limit', 'cu_price', 'core', 'tip']
    : ['cu_limit', 'cu_price', 'create_ata', 'core', 'tip'];
}

interface Props {
  label: string;
  /** Current authored steps, or undefined ⇒ the canonical default (nothing authored). */
  value: DecoStep[] | undefined;
  onChange: (steps: DecoStep[] | undefined) => void;
  kind: LayoutKind;
  /** A buy leg is a snipe (forces CU price + Tip); a create is not. */
  isSnipe?: boolean;
}

export function IxLayoutEditor({ label, value, onChange, kind, isSnipe = kind === 'buy' }: Props) {
  const authored = value !== undefined;
  const steps = value ?? canonicalSteps(kind);
  const error = useMemo(() => validateLayout(steps, kind, isSnipe), [steps, kind, isSnipe]);

  // Steps not yet in the layout, addable from the menu. `create_ata` is never
  // offered for a create layout (the buyer ATA is fused into Core there).
  const absent = ALL_STEPS.filter(
    (s) => !steps.includes(s) && !(kind === 'create' && s === 'create_ata'),
  );

  const set = (next: DecoStep[]) => onChange(next);
  const move = (i: number, dir: -1 | 1) => {
    const j = i + dir;
    if (j < 0 || j >= steps.length) return;
    const next = steps.slice();
    [next[i], next[j]] = [next[j], next[i]];
    set(next);
  };
  const remove = (i: number) => {
    if (steps[i] === 'core') return; // Core is required — never removable.
    set(steps.filter((_, idx) => idx !== i));
  };
  const add = (s: DecoStep) => set([...steps, s]);

  return (
    <Field label={label}>
      {!authored ? (
        <div className="flex items-center gap-2">
          <span className="text-xs muted">Canonical default ({steps.map((s) => STEP_LABEL[s]).join(' · ')})</span>
          <Button size="sm" onClick={() => onChange(canonicalSteps(kind))}>Customize</Button>
        </div>
      ) : (
        <div className="space-y-2">
          <div className="flex flex-wrap items-center gap-1">
            {steps.map((s, i) => (
              <span
                key={`${s}-${i}`}
                className={clsx(
                  'inline-flex items-center gap-1 rounded border px-1.5 py-0.5 text-xs',
                  s === 'core' ? 'border-[var(--color-accent)] font-semibold' : 'border-[var(--color-border)]',
                )}
              >
                {STEP_LABEL[s]}
                <button type="button" className="muted hover:opacity-100" title="Move left"
                  onClick={() => move(i, -1)} disabled={i === 0}>↑</button>
                <button type="button" className="muted hover:opacity-100" title="Move right"
                  onClick={() => move(i, 1)} disabled={i === steps.length - 1}>↓</button>
                {s !== 'core' && (
                  <button type="button" className="text-[var(--color-bad)] hover:opacity-100" title="Remove"
                    onClick={() => remove(i)}>×</button>
                )}
              </span>
            ))}
          </div>
          <div className="flex flex-wrap items-center gap-1">
            {absent.map((s) => (
              <Button key={s} size="sm" variant="ghost" onClick={() => add(s)}>+ {STEP_LABEL[s]}</Button>
            ))}
            <Button size="sm" variant="ghost" onClick={() => onChange(undefined)}>Reset to canonical</Button>
          </div>
          {error && <Badge tone="bad">{error}</Badge>}
        </div>
      )}
    </Field>
  );
}
