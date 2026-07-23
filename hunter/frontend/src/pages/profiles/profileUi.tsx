import type { ProfileType, WalletProfileTag } from 'types';
import { Badge, type BadgeVariant } from 'components/ui/Badge';

export const TYPE_BADGE: Record<ProfileType, BadgeVariant> = {
  mine: 'primary',
  trader: 'info',
  whale: 'warning',
  dev: 'accent',
};

/** Tag swatches aligned to `index.css` theme tokens (stored as hex on the tag). */
export const PRESET_COLORS = [
  '#f23645', // danger / red
  '#f69768', // accent
  '#f4e07a', // warning
  '#eed35a', // secondary
  '#089981', // green
  '#13ceaf', // primary (live teal)
  '#06b6d4', // lab cyan
  '#4c7ef0', // info
  '#a855f7', // purple (no theme twin — keeps picker breadth)
  '#999999', // text-dim
];

export function TypeBadge({ type }: { type: ProfileType }) {
  return (
    <Badge variant={TYPE_BADGE[type]} size="sm" pill>
      {type}
    </Badge>
  );
}

export function shortAddr(address: string) {
  return `${address.slice(0, 6)}…${address.slice(-4)}`;
}

interface TagChipProps {
  tag: WalletProfileTag;
  onRemove?: () => void;
}

export function TagChip({ tag, onRemove }: TagChipProps) {
  return (
    <span
      className="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium"
      style={{
        backgroundColor: `${tag.color}22`,
        color: tag.color,
        border: `1px solid ${tag.color}55`,
      }}
    >
      <span
        className="inline-block h-1.5 w-1.5 shrink-0 rounded-full"
        style={{ backgroundColor: tag.color }}
      />
      {tag.name}
      {onRemove && (
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            onRemove();
          }}
          className="ml-0.5 leading-none opacity-60 hover:opacity-100"
          title="Remove tag"
        >
          ×
        </button>
      )}
    </span>
  );
}

interface ColorPickerProps {
  value: string;
  onChange: (color: string) => void;
}

export function ColorPicker({ value, onChange }: ColorPickerProps) {
  return (
    <div className="flex flex-wrap gap-1.5">
      {PRESET_COLORS.map((c) => (
        <button
          key={c}
          type="button"
          onClick={() => onChange(c)}
          className="h-5 w-5 rounded-full border-2 transition-transform hover:scale-110"
          style={{
            backgroundColor: c,
            borderColor: value === c ? '#fff' : 'transparent',
            outline: value === c ? `2px solid ${c}` : 'none',
            outlineOffset: '1px',
          }}
          title={c}
        />
      ))}
    </div>
  );
}
