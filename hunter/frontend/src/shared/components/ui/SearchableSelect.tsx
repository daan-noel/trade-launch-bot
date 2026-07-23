// A `<select>` replacement that filters its options as you type — for lists
// long enough that scrolling a native dropdown gets tedious (e.g. picking one
// saved fingerprint out of dozens). Generic over the option's payload `T` so
// callers keep their own domain type (no string-only enum needed).
//
// Kept intentionally single-purpose: substring match (case-insensitive) on
// `label`, arrow-key navigation, Enter to commit, Escape/click-outside to
// close without committing. Not a general-purpose multiselect/autocomplete —
// if a second call site needs different matching or multi-value support,
// extend this one rather than forking it.

import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from 'react';
import { cn } from 'lib/cn';
import { fieldClassName, type FieldProps } from './Input';
import { CloseIcon, SearchIcon } from './icons';

export interface SearchableSelectOption<T> {
  value: string;
  label: string;
  data: T;
}

interface SearchableSelectProps<T> extends FieldProps {
  options: SearchableSelectOption<T>[];
  /** Selected option's `value`, or `null`/`''` for none. */
  value: string | null;
  /** `''` when the user picks the (optional) `emptyOptionLabel` row. */
  onChange: (value: string) => void;
  /** Shown as the input's placeholder when nothing is selected. */
  placeholder?: string;
  /** An always-visible, unfiltered first row (e.g. "Manual — clear scope").
   *  Selecting it calls `onChange('')`. Omit to require picking a real option. */
  emptyOptionLabel?: string;
  disabled?: boolean;
  /** Custom row renderer (falls back to plain text). Receives the option and
   *  whether it's the keyboard-highlighted row. */
  renderOption?: (opt: SearchableSelectOption<T>, highlighted: boolean) => ReactNode;
  className?: string;
  noResultsLabel?: string;
}

/** Case/diacritic-insensitive-enough substring match — good enough for names
 *  and short ids; not meant to be a fuzzy matcher. */
function matches(label: string, query: string): boolean {
  return label.toLowerCase().includes(query.trim().toLowerCase());
}

export function SearchableSelect<T>({
  options,
  value,
  onChange,
  placeholder = 'Search…',
  emptyOptionLabel,
  disabled,
  renderOption,
  className,
  noResultsLabel = 'No matches',
  fieldSize = 'sm',
  variant = 'default',
}: SearchableSelectProps<T>) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [highlighted, setHighlighted] = useState(0);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);

  const selected = useMemo(() => options.find((o) => o.value === value) ?? null, [options, value]);

  const filtered = useMemo(
    () => (query.trim() ? options.filter((o) => matches(o.label, query)) : options),
    [options, query],
  );

  // Rows the keyboard cursor can land on: the empty/clear row (if any) is
  // always first and unfiltered, then the (possibly filtered) real options.
  const rows: Array<{ kind: 'empty' } | { kind: 'option'; opt: SearchableSelectOption<T> }> = useMemo(() => {
    const out: Array<{ kind: 'empty' } | { kind: 'option'; opt: SearchableSelectOption<T> }> = [];
    if (emptyOptionLabel != null) out.push({ kind: 'empty' });
    for (const opt of filtered) out.push({ kind: 'option', opt });
    return out;
  }, [emptyOptionLabel, filtered]);

  useEffect(() => {
    if (highlighted >= rows.length) setHighlighted(Math.max(0, rows.length - 1));
  }, [rows.length, highlighted]);

  // Click-outside closes without committing the in-progress search text.
  useEffect(() => {
    if (!open) return;
    function onDocMouseDown(e: MouseEvent) {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpen(false);
        setQuery('');
      }
    }
    document.addEventListener('mousedown', onDocMouseDown);
    return () => document.removeEventListener('mousedown', onDocMouseDown);
  }, [open]);

  function openList() {
    if (disabled) return;
    setOpen(true);
    setQuery('');
    setHighlighted(0);
  }

  function commit(row: { kind: 'empty' } | { kind: 'option'; opt: SearchableSelectOption<T> }) {
    onChange(row.kind === 'empty' ? '' : row.opt.value);
    setOpen(false);
    setQuery('');
    inputRef.current?.blur();
  }

  function onKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (!open) {
      if (e.key === 'ArrowDown' || e.key === 'Enter') {
        e.preventDefault();
        openList();
      }
      return;
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setHighlighted((i) => Math.min(i + 1, rows.length - 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setHighlighted((i) => Math.max(i - 1, 0));
    } else if (e.key === 'Enter') {
      e.preventDefault();
      const row = rows[highlighted];
      if (row) commit(row);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      setOpen(false);
      setQuery('');
      inputRef.current?.blur();
    }
  }

  const displayValue = open ? query : selected?.label ?? '';
  const showPlaceholder = open ? filtered.length === 0 && query === '' : !selected;

  return (
    <div ref={containerRef} className="relative">
      <div className="relative">
        <input
          ref={inputRef}
          type="text"
          role="combobox"
          aria-expanded={open}
          aria-autocomplete="list"
          disabled={disabled}
          value={displayValue}
          placeholder={showPlaceholder ? placeholder : undefined}
          onFocus={openList}
          onClick={openList}
          onChange={(e) => {
            if (!open) setOpen(true);
            setQuery(e.target.value);
            setHighlighted(0);
          }}
          onKeyDown={onKeyDown}
          className={fieldClassName({ size: fieldSize, variant, className: cn('pr-6', className) })}
        />
        <span className="pointer-events-none absolute right-1.5 top-1/2 -translate-y-1/2 text-text-dim/50">
          {value ? (
            <button
              type="button"
              tabIndex={-1}
              className="pointer-events-auto rounded hover:text-text-dim"
              title="Clear"
              onClick={(e) => {
                e.stopPropagation();
                onChange('');
                setOpen(false);
                setQuery('');
              }}
            >
              <CloseIcon className="h-3 w-3" />
            </button>
          ) : (
            <SearchIcon className="h-3 w-3" />
          )}
        </span>
      </div>
      {open && (
        <div className="absolute left-0 right-0 top-full z-20 mt-1 max-h-64 overflow-y-auto rounded-md border border-white/10 bg-bg-card py-1 shadow-lg">
          {rows.length === 0 ? (
            <div className="px-2.5 py-1.5 text-[11px] text-text-dim/60">{noResultsLabel}</div>
          ) : (
            rows.map((row, i) => {
              const isHighlighted = i === highlighted;
              const isSelected = row.kind === 'option' && row.opt.value === value;
              return (
                <div
                  key={row.kind === 'empty' ? '__empty__' : row.opt.value}
                  role="option"
                  aria-selected={isSelected}
                  onMouseEnter={() => setHighlighted(i)}
                  onMouseDown={(e) => {
                    // mousedown (not click) so this fires before the input's
                    // blur/click-outside handler can close the list first.
                    e.preventDefault();
                    commit(row);
                  }}
                  className={cn(
                    'cursor-pointer px-2.5 py-1.5 text-[11px] leading-tight',
                    isHighlighted ? 'bg-primary/15 text-text' : 'text-text-mid hover:bg-white/5',
                    isSelected && 'font-medium text-text',
                  )}
                >
                  {row.kind === 'empty'
                    ? emptyOptionLabel
                    : renderOption
                      ? renderOption(row.opt, isHighlighted)
                      : row.opt.label}
                </div>
              );
            })
          )}
        </div>
      )}
    </div>
  );
}
