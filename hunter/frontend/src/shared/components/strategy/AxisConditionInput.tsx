import { useEffect, useRef, useState } from 'react';

import { Input } from 'components/ui/Input';
import { cn } from 'lib/cn';
import {
  formatPredicate,
  type AxisDef,
  type AxisPredicate,
} from 'lib/strategy/fingerprintAxes';
import {
  formatAxisPredicate,
  parseAxisPredicate,
  parseSpanSet,
} from 'lib/strategy/fingerprintGrammar';

export interface AxisConditionInputProps {
  def: AxisDef;
  /** The expression as the operator typed it (controlled by the form). */
  value: string;
  onChange: (text: string) => void;
  disabled?: boolean;
  title?: string;
  className?: string;
}

/** What the typed text currently means. Three failures, not one: they need
 *  different sentences, because "you mistyped" and "you asked for a gate no token
 *  can pass" are different mistakes and the second is the dangerous one. */
export type AxisConditionState =
  | { kind: 'unset' }
  | { kind: 'ok'; predicate: AxisPredicate }
  | { kind: 'malformed' }
  | { kind: 'empty' }
  | { kind: 'unconstrained' };

/**
 * Read one axis's typed expression. **The one interpreter**, exported so the form
 * builds its criteria from exactly what this input renders — a second reader would
 * let the chips and the saved row disagree.
 */
export function axisConditionState(text: string, def: AxisDef): AxisConditionState {
  if (text.trim() === '') return { kind: 'unset' };
  const spans = parseSpanSet(text, def.unit);
  if (spans == null) return { kind: 'malformed' };
  const predicate = parseAxisPredicate(text, def.unit);
  if (predicate) return { kind: 'ok', predicate };
  // Parsed, but selects nothing (`<=2, >=7`) or everything (`>=0`). Neither can be
  // stored: one is a gate that never fires, the other reads as narrowed while
  // matching every token.
  return spans.length === 0 ? { kind: 'empty' } : { kind: 'unconstrained' };
}

/** The operator-facing sentence for a state that cannot be saved, or `null`. */
export function axisConditionProblem(state: AxisConditionState, def: AxisDef): string | null {
  switch (state.kind) {
    case 'malformed':
      return `${def.label}: not a condition — try 3, 1..5, >=2, <=9, !=3 or <=2 | >=7`;
    case 'empty':
      return `${def.label}: no value can satisfy that — the arms exclude each other`;
    case 'unconstrained':
      return `${def.label}: that matches every token, so it configures nothing — clear it or narrow it`;
    default:
      return null;
  }
}

/**
 * One axis's condition editor: a grammar input (`3`, `1..5`, `>=2`, `!=3`,
 * `<=2 | >=7`), the parsed predicate echoed beneath it, and a red underline plus a
 * unit-aware hint when the text does not say something storable.
 *
 * Text-first: the field holds the raw string while editing and normalises to the
 * canonical spelling on blur, so a half-typed `1.` is never rounded mid-keystroke
 * and a half-open `1.5-1.6` visibly resolves to the inclusive window it means.
 */
export function AxisConditionInput({
  def,
  value,
  onChange,
  disabled,
  title,
  className,
}: AxisConditionInputProps) {
  const [focused, setFocused] = useState(false);
  const lastValue = useRef(value);
  const [text, setText] = useState(value);

  useEffect(() => {
    if (!focused && value !== lastValue.current) setText(value);
    lastValue.current = value;
  }, [value, focused]);

  const state = axisConditionState(text, def);
  const invalid = state.kind !== 'unset' && state.kind !== 'ok';
  const suffix = def.unit === 'lamports' ? '◎' : undefined;

  return (
    <div className={cn('flex flex-col gap-1', className)}>
      <Input
        fieldSize="sm"
        className={cn('min-w-0 flex-1', invalid && 'border-red/70 focus:border-red')}
        placeholder={def.unit === 'lamports' ? '1.5  ·  1.5..2  ·  >=1.5' : '3  ·  1..5  ·  !=3'}
        unit={suffix}
        aria-invalid={invalid}
        disabled={disabled}
        title={title}
        value={text}
        onChange={(e) => {
          setText(e.target.value);
          onChange(e.target.value);
        }}
        onFocus={() => setFocused(true)}
        onBlur={() => {
          setFocused(false);
          // Snap to the canonical spelling, so what the field shows is what a
          // re-parse would produce — and so two operators typing the same gate two
          // ways see they wrote the same thing.
          if (state.kind === 'ok') {
            const canonical = formatAxisPredicate(state.predicate, def.unit);
            setText(canonical);
            if (canonical !== text) onChange(canonical);
          }
        }}
      />
      {state.kind === 'ok' ? (
        <span className="font-mono text-[11px] text-text-dim">
          {formatPredicate(def.id, state.predicate)}
          {suffix ?? ''}
        </span>
      ) : invalid ? (
        <span className="text-[11px] text-red">{axisConditionProblem(state, def)}</span>
      ) : null}
    </div>
  );
}
