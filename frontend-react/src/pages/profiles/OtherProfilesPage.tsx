import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useDispatch } from 'react-redux';
import {
  createProfile,
  createTag,
  createWalletEntry,
  deleteProfile,
  deleteTag,
  deleteWalletEntry,
  fetchTags,
  updateProfile,
  updateProfileTags,
  updateTag,
  updateWalletEntry,
} from 'services/api';
import { apiErrorMessage, useGetProfilesQuery } from 'store/apiSlice';
import { sharedApi } from 'store/sharedEndpoints';
import type { AppDispatch } from 'store/types';
import type { ProfileType, WalletEntry, WalletProfile, WalletProfileTag } from 'types';
import { Badge, type BadgeVariant } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { Input, Textarea } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { InlineAlert, Modal } from 'components/ui/Modal';
import { useTimezone } from 'context/TimezoneContext';
import { formatIsoLines } from 'utils/date';

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

// Stable empty fallback so `useGetProfilesQuery`'s default never creates a new
// array reference on each render (matches the other profile consumers).
const EMPTY_PROFILES: WalletProfile[] = [];

const PROFILE_TYPES: ProfileType[] = ['trader', 'whale', 'dev'];

const TYPE_BADGE: Record<ProfileType, BadgeVariant> = {
  mine: 'primary',
  trader: 'info',
  whale: 'warning',
  dev: 'accent',
};

const PRESET_COLORS = [
  '#ef4444', // red
  '#f97316', // orange
  '#eab308', // yellow
  '#22c55e', // green
  '#14b8a6', // teal
  '#3b82f6', // blue
  '#6366f1', // indigo
  '#a855f7', // purple
  '#ec4899', // pink
  '#6b7280', // gray
];

// ---------------------------------------------------------------------------
// Small shared components
// ---------------------------------------------------------------------------

function TypeBadge({ type }: { type: ProfileType }) {
  return (
    <Badge variant={TYPE_BADGE[type]} size="sm" pill>
      {type}
    </Badge>
  );
}

function shortAddr(address: string) {
  return `${address.slice(0, 6)}â€¦${address.slice(-4)}`;
}

interface TagChipProps {
  tag: WalletProfileTag;
  onRemove?: () => void;
}

function TagChip({ tag, onRemove }: TagChipProps) {
  return (
    <span
      className="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium"
      style={{ backgroundColor: `${tag.color}22`, color: tag.color, border: `1px solid ${tag.color}55` }}
    >
      <span
        className="inline-block w-1.5 h-1.5 rounded-full shrink-0"
        style={{ backgroundColor: tag.color }}
      />
      {tag.name}
      {onRemove && (
        <button
          onClick={(e) => { e.stopPropagation(); onRemove(); }}
          className="ml-0.5 opacity-60 hover:opacity-100 leading-none"
          title="Remove tag"
        >
          Ã—
        </button>
      )}
    </span>
  );
}

interface ColorPickerProps {
  value: string;
  onChange: (color: string) => void;
}

function ColorPicker({ value, onChange }: ColorPickerProps) {
  return (
    <div className="flex gap-1.5 flex-wrap">
      {PRESET_COLORS.map((c) => (
        <button
          key={c}
          type="button"
          onClick={() => onChange(c)}
          className="w-5 h-5 rounded-full border-2 transition-transform hover:scale-110"
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

// ---------------------------------------------------------------------------
// Manage Tags modal
// ---------------------------------------------------------------------------

interface ManageTagsModalProps {
  open: boolean;
  tags: WalletProfileTag[];
  onClose: () => void;
  onCreated: (tag: WalletProfileTag) => void;
  onUpdated: (tag: WalletProfileTag) => void;
  onDeleted: (id: string) => void;
}

function ManageTagsModal({ open, tags, onClose, onCreated, onUpdated, onDeleted }: ManageTagsModalProps) {
  // New tag form
  const [newName, setNewName] = useState('');
  const [newColor, setNewColor] = useState(PRESET_COLORS[6]);
  const [newComment, setNewComment] = useState('');
  const [adding, setAdding] = useState(false);
  const [addError, setAddError] = useState('');

  // Inline edit state: tagId -> { name, color, comment }
  const [editing, setEditing] = useState<Record<string, { name: string; color: string; comment: string }>>({});
  const [saving, setSaving] = useState<string | null>(null);
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null);

  useEffect(() => {
    if (open) {
      setNewName(''); setNewColor(PRESET_COLORS[6]); setNewComment('');
      setAddError(''); setEditing({}); setDeleteConfirm(null);
    }
  }, [open]);

  const startEdit = (tag: WalletProfileTag) => {
    setEditing((prev) => ({
      ...prev,
      [tag.id]: { name: tag.name, color: tag.color, comment: tag.comment ?? '' },
    }));
  };

  const cancelEdit = (id: string) => {
    setEditing((prev) => { const next = { ...prev }; delete next[id]; return next; });
  };

  const saveEdit = async (tag: WalletProfileTag) => {
    const draft = editing[tag.id];
    if (!draft || !draft.name.trim()) return;
    setSaving(tag.id);
    try {
      await updateTag(tag.id, { name: draft.name.trim(), color: draft.color, comment: draft.comment.trim() || undefined });
      onUpdated({ ...tag, name: draft.name.trim(), color: draft.color, comment: draft.comment.trim() || null });
      cancelEdit(tag.id);
    } catch {
      // keep editing open on error
    } finally {
      setSaving(null);
    }
  };

  const handleAdd = async () => {
    if (!newName.trim()) { setAddError('Name is required'); return; }
    setAdding(true); setAddError('');
    try {
      const tag = await createTag({ name: newName.trim(), color: newColor, comment: newComment.trim() || undefined });
      onCreated(tag);
      setNewName(''); setNewComment(''); setNewColor(PRESET_COLORS[6]);
      setTimeout(scrollToBottom, 50);
    } catch (e) {
      setAddError(e instanceof Error ? e.message : 'Failed to create tag');
    } finally {
      setAdding(false);
    }
  };

  const handleDelete = async (id: string) => {
    try {
      await deleteTag(id);
      onDeleted(id);
      setDeleteConfirm(null);
    } catch {
      // silent â€” tag stays
    }
  };

  const listEndRef = useRef<HTMLDivElement>(null);

  const scrollToBottom = useCallback(() => {
    listEndRef.current?.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
  }, []);

  useEffect(() => {
    if (open) scrollToBottom();
  }, [tags.length, open, scrollToBottom]);

  return (
    <Modal title="Manage tags" open={open} onClose={onClose}>
      <div className="flex flex-col gap-0 min-w-[360px]">
        {/* Existing tags */}
        {tags.length === 0 ? (
          <p className="text-xs text-text-dim px-1 pb-3">No tags yet. Create one below.</p>
        ) : (
          <div className="flex flex-col gap-1 pb-3 max-h-[240px] overflow-y-auto pr-1">
            {tags.map((tag) => {
              const draft = editing[tag.id];
              return (
                <div key={tag.id} className="rounded-lg border border-white/8 bg-white/2 px-3 py-2">
                  {draft ? (
                    /* inline edit */
                    <div className="flex flex-col gap-2">
                      <div className="flex gap-2">
                        <Input
                          value={draft.name}
                          onChange={(e) => setEditing((p) => ({ ...p, [tag.id]: { ...p[tag.id], name: e.target.value } }))}
                          fieldSize="sm"
                          className="flex-1"
                          onKeyDown={(e) => { if (e.key === 'Enter') saveEdit(tag); if (e.key === 'Escape') cancelEdit(tag.id); }}
                          autoFocus
                        />
                        <Button size="xs" variant="primary" onClick={() => saveEdit(tag)} disabled={saving === tag.id}>
                          {saving === tag.id ? 'â€¦' : 'Save'}
                        </Button>
                        <Button size="xs" variant="ghost" onClick={() => cancelEdit(tag.id)}>Cancel</Button>
                      </div>
                      <ColorPicker
                        value={draft.color}
                        onChange={(c) => setEditing((p) => ({ ...p, [tag.id]: { ...p[tag.id], color: c } }))}
                      />
                      <Input
                        value={draft.comment}
                        onChange={(e) => setEditing((p) => ({ ...p, [tag.id]: { ...p[tag.id], comment: e.target.value } }))}
                        placeholder="Note (optional)"
                        fieldSize="sm"
                      />
                    </div>
                  ) : deleteConfirm === tag.id ? (
                    /* delete confirm inline */
                    <div className="flex items-center gap-2">
                      <span className="text-xs text-text-dim flex-1">Delete <span className="font-semibold text-text">{tag.name}</span>?</span>
                      <Button size="xs" variant="danger" onClick={() => handleDelete(tag.id)}>Delete</Button>
                      <Button size="xs" variant="ghost" onClick={() => setDeleteConfirm(null)}>Cancel</Button>
                    </div>
                  ) : (
                    /* view row */
                    <div className="flex items-center gap-2">
                      <span
                        className="w-3 h-3 rounded-full shrink-0"
                        style={{ backgroundColor: tag.color }}
                      />
                      <span className="text-sm font-medium text-text flex-1">{tag.name}</span>
                      {tag.comment && (
                        <span className="text-xs text-text-dim truncate max-w-[120px]" title={tag.comment}>
                          {tag.comment}
                        </span>
                      )}
                      <Button size="xs" variant="ghost" onClick={() => startEdit(tag)}>Edit</Button>
                      <Button size="xs" variant="danger" onClick={() => setDeleteConfirm(tag.id)}>âœ•</Button>
                    </div>
                  )}
                </div>
              );
            })}
            <div ref={listEndRef} />
          </div>
        )}

        {/* Divider + new tag form */}
        <div className="border-t border-white/8 pt-3 flex flex-col gap-2">
          <p className="text-xs font-semibold text-text-dim uppercase tracking-wide">New tag</p>
          <Input
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            placeholder="Tag name"
            fieldSize="sm"
            onKeyDown={(e) => e.key === 'Enter' && handleAdd()}
          />
          <ColorPicker value={newColor} onChange={setNewColor} />
          <Input
            value={newComment}
            onChange={(e) => setNewComment(e.target.value)}
            placeholder="Note (optional)"
            fieldSize="sm"
          />
          {addError && <InlineAlert variant="error">{addError}</InlineAlert>}
          <div className="flex justify-end">
            <Button size="sm" variant="primary" onClick={handleAdd} disabled={adding}>
              {adding ? 'Addingâ€¦' : '+ Add tag'}
            </Button>
          </div>
        </div>
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------------------
// Profile tag assignment dropdown
// ---------------------------------------------------------------------------

interface TagAssignDropdownProps {
  profile: WalletProfile;
  allTags: WalletProfileTag[];
  onToggle: (tag: WalletProfileTag, assign: boolean) => void;
}

function TagAssignDropdown({ profile, allTags, onToggle }: TagAssignDropdownProps) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [open]);

  const unassigned = allTags.filter((t) => !profile.tags.some((pt) => pt.id === t.id));

  if (allTags.length === 0) return null;

  return (
    <div className="relative" ref={ref}>
      <button
        onClick={() => setOpen((v) => !v)}
        className="inline-flex items-center gap-0.5 rounded-full border border-white/15 px-2 py-0.5 text-xs text-text-dim hover:text-text hover:border-white/30 transition-colors"
      >
        + tag
      </button>
      {open && (
        <div className="absolute left-0 top-full mt-1 z-50 min-w-[140px] rounded-lg border border-white/12 bg-bg-panel shadow-xl py-1 max-h-[200px] overflow-y-auto">
          {unassigned.length === 0 ? (
            <p className="px-3 py-1.5 text-xs text-text-dim">All tags assigned</p>
          ) : (
            unassigned.map((tag) => (
              <button
                key={tag.id}
                onClick={() => { onToggle(tag, true); setOpen(false); }}
                className="flex items-center gap-2 w-full px-3 py-1.5 text-xs text-left hover:bg-white/5 transition-colors"
              >
                <span className="w-2.5 h-2.5 rounded-full shrink-0" style={{ backgroundColor: tag.color }} />
                {tag.name}
              </button>
            ))
          )}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Create / Edit Profile modal
// ---------------------------------------------------------------------------

interface ProfileModalProps {
  open: boolean;
  initial?: WalletProfile;
  onClose: () => void;
  onSaved: (profile: WalletProfile) => void;
}

function ProfileModal({ open, initial, onClose, onSaved }: ProfileModalProps) {
  const [name, setName] = useState(initial?.name ?? '');
  const [type, setType] = useState<ProfileType>(initial?.profile_type ?? 'trader');
  const [error, setError] = useState('');
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (open) {
      setName(initial?.name ?? '');
      setType(initial?.profile_type ?? 'trader');
      setError('');
    }
  }, [open, initial]);

  const submit = async () => {
    if (!name.trim()) { setError('Name is required'); return; }
    setSaving(true);
    setError('');
    try {
      if (initial) {
        await updateProfile(initial.id, { name: name.trim(), profile_type: type });
        onSaved({ ...initial, name: name.trim(), profile_type: type });
      } else {
        const created = await createProfile({ name: name.trim(), profile_type: type });
        onSaved({ ...created, wallets: [], tags: [] });
      }
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to save');
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal title={initial ? 'Edit profile' : 'New profile'} open={open} onClose={onClose}>
      <div className="flex flex-col gap-3">
        <label className="flex flex-col gap-1.5">
          <span className="text-xs text-text-dim">Name</span>
          <Input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="e.g. Alpha traders"
            fieldSize="md"
            onKeyDown={(e) => e.key === 'Enter' && submit()}
          />
        </label>
        <label className="flex flex-col gap-1.5">
          <span className="text-xs text-text-dim">Type</span>
          <Select value={type} onChange={(e) => setType(e.target.value as ProfileType)} fieldSize="md">
            {PROFILE_TYPES.map((t) => (
              <option key={t} value={t}>{t}</option>
            ))}
          </Select>
        </label>
        {error && <InlineAlert variant="error">{error}</InlineAlert>}
        <div className="flex justify-end gap-2 pt-1">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={submit} disabled={saving}>
            {saving ? 'Savingâ€¦' : 'Save'}
          </Button>
        </div>
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------------------
// Add Wallet modal
// ---------------------------------------------------------------------------

interface AddWalletModalProps {
  open: boolean;
  profileId: string;
  onClose: () => void;
  onAdded: (wallet: WalletEntry) => void;
}

function AddWalletModal({ open, profileId, onClose, onAdded }: AddWalletModalProps) {
  const [address, setAddress] = useState('');
  const [comment, setComment] = useState('');
  const [tracked, setTracked] = useState(true);
  const [error, setError] = useState('');
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (open) { setAddress(''); setComment(''); setTracked(true); setError(''); }
  }, [open]);

  const submit = async () => {
    if (!address.trim()) { setError('Address is required'); return; }
    setSaving(true);
    setError('');
    try {
      const wallet = await createWalletEntry(profileId, {
        address: address.trim(),
        is_tracked: tracked,
        comment: comment.trim() || undefined,
      });
      onAdded(wallet);
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to add wallet');
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal title="Add wallet" open={open} onClose={onClose}>
      <div className="flex flex-col gap-3">
        <label className="flex flex-col gap-1.5">
          <span className="text-xs text-text-dim">Wallet address</span>
          <Input
            value={address}
            onChange={(e) => setAddress(e.target.value)}
            placeholder="Solana base58 address"
            fieldSize="md"
          />
        </label>
        <label className="flex flex-col gap-1.5">
          <span className="text-xs text-text-dim">Comment (optional)</span>
          <Input
            value={comment}
            onChange={(e) => setComment(e.target.value)}
            placeholder="e.g. Insider wallet"
            fieldSize="md"
          />
        </label>
        <label className="flex items-center gap-2 cursor-pointer">
          <input
            type="checkbox"
            checked={tracked}
            onChange={(e) => setTracked(e.target.checked)}
            className="accent-primary"
          />
          <span className="text-xs text-text-dim">Track this wallet</span>
        </label>
        {error && <InlineAlert variant="error">{error}</InlineAlert>}
        <div className="flex justify-end gap-2 pt-1">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={submit} disabled={saving}>
            {saving ? 'Addingâ€¦' : 'Add wallet'}
          </Button>
        </div>
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------------------
// Edit Wallet modal
// ---------------------------------------------------------------------------

interface EditWalletModalProps {
  open: boolean;
  wallet: WalletEntry | null;
  onClose: () => void;
  onSaved: (wallet: WalletEntry) => void;
}

function EditWalletModal({ open, wallet, onClose, onSaved }: EditWalletModalProps) {
  const [comment, setComment] = useState('');
  const [tracked, setTracked] = useState(true);
  const [error, setError] = useState('');
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (open && wallet) {
      setComment(wallet.comment ?? '');
      setTracked(wallet.is_tracked);
      setError('');
    }
  }, [open, wallet]);

  const submit = async () => {
    if (!wallet) return;
    setSaving(true);
    setError('');
    try {
      await updateWalletEntry(wallet.id, {
        is_tracked: tracked,
        comment: comment.trim() || null,
      });
      onSaved({ ...wallet, is_tracked: tracked, comment: comment.trim() || null });
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to save');
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal title="Edit wallet" open={open} onClose={onClose}>
      <div className="flex flex-col gap-3">
        <div className="rounded-md border border-white/8 bg-white/3 px-3 py-2 font-mono text-xs text-text-dim break-all">
          {wallet?.address}
        </div>
        <label className="flex flex-col gap-1.5">
          <span className="text-xs text-text-dim">Comment</span>
          <Textarea
            value={comment}
            onChange={(e) => setComment(e.target.value)}
            placeholder="Notes about this wallet"
            fieldSize="md"
          />
        </label>
        <label className="flex items-center gap-2 cursor-pointer">
          <input
            type="checkbox"
            checked={tracked}
            onChange={(e) => setTracked(e.target.checked)}
            className="accent-primary"
          />
          <span className="text-xs text-text-dim">Track this wallet</span>
        </label>
        {error && <InlineAlert variant="error">{error}</InlineAlert>}
        <div className="flex justify-end gap-2 pt-1">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={submit} disabled={saving}>
            {saving ? 'Savingâ€¦' : 'Save'}
          </Button>
        </div>
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------------------
// Delete confirm modal
// ---------------------------------------------------------------------------

interface ConfirmDeleteModalProps {
  open: boolean;
  label: string;
  onClose: () => void;
  onConfirm: () => Promise<void>;
}

function ConfirmDeleteModal({ open, label, onClose, onConfirm }: ConfirmDeleteModalProps) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => { if (open) { setBusy(false); setError(''); } }, [open]);

  const go = async () => {
    setBusy(true);
    setError('');
    try {
      await onConfirm();
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to delete');
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal title="Confirm delete" open={open} onClose={onClose}>
      <div className="flex flex-col gap-3">
        <p className="text-sm text-text-dim">
          Delete <span className="font-semibold text-text">{label}</span>? This cannot be undone.
        </p>
        {error && <InlineAlert variant="error">{error}</InlineAlert>}
        <div className="flex justify-end gap-2 pt-1">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button variant="danger" onClick={go} disabled={busy}>
            {busy ? 'Deletingâ€¦' : 'Delete'}
          </Button>
        </div>
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------------------
// Wallet row
// ---------------------------------------------------------------------------

interface WalletRowProps {
  wallet: WalletEntry;
  onEdit: (w: WalletEntry) => void;
  onDelete: (w: WalletEntry) => void;
  onToggleTracked: (w: WalletEntry) => void;
  toggling?: boolean;
}

function WalletRow({ wallet, onEdit, onDelete, onToggleTracked, toggling }: WalletRowProps) {
  const { timezone } = useTimezone();
  return (
    <div className="flex items-center gap-3 rounded-md border border-white/6 bg-white/2 px-3 py-2 text-xs">
      <span
        className="font-mono text-text-dim min-w-0 shrink truncate"
        title={wallet.address}
      >
        {shortAddr(wallet.address)}
      </span>
      <span className="hidden sm:block flex-1 min-w-0 truncate text-text-dim italic">
        {wallet.comment ?? ''}
      </span>
      <button
        onClick={() => onToggleTracked(wallet)}
        disabled={toggling}
        title={wallet.is_tracked ? 'Tracked â€” click to untrack' : 'Untracked â€” click to track'}
        className={[
          'flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors select-none',
          toggling ? 'opacity-40 cursor-not-allowed' : 'cursor-pointer hover:bg-white/8',
          wallet.is_tracked ? 'text-primary' : 'text-text-dim',
        ].join(' ')}
      >
        {wallet.is_tracked ? (
          /* eye icon */
          <svg xmlns="http://www.w3.org/2000/svg" className="w-3.5 h-3.5" viewBox="0 0 20 20" fill="currentColor">
            <path d="M10 3C5 3 1.73 7.11 1.05 9.78a1 1 0 000 .44C1.73 12.89 5 17 10 17s8.27-4.11 8.95-6.78a1 1 0 000-.44C18.27 7.11 15 3 10 3zm0 11a4 4 0 110-8 4 4 0 010 8z" />
            <path d="M10 8a2 2 0 100 4 2 2 0 000-4z" />
          </svg>
        ) : (
          /* eye-off icon */
          <svg xmlns="http://www.w3.org/2000/svg" className="w-3.5 h-3.5" viewBox="0 0 20 20" fill="currentColor">
            <path fillRule="evenodd" d="M3.28 2.22a.75.75 0 00-1.06 1.06l14.5 14.5a.75.75 0 101.06-1.06l-1.745-1.745a10.029 10.029 0 003.3-4.38 1 1 0 000-.71C18.27 7.11 15 3 10 3a9.956 9.956 0 00-4.95 1.32L3.28 2.22zM10 5c.538 0 1.064.058 1.57.167L5.12 11.617A4 4 0 0110 5zm-4.873 9.32L3.28 16.167A.75.75 0 104.34 17.22l.967-.967A9.956 9.956 0 0010 17c5 0 8.27-4.11 8.95-6.78a1 1 0 000-.71 10.049 10.049 0 00-2.354-3.583l-1.49-1.49A4 4 0 0110.002 14a3.994 3.994 0 01-4.875-.68z" clipRule="evenodd" />
          </svg>
        )}
        <span>{wallet.is_tracked ? 'tracked' : 'untracked'}</span>
      </button>
      {wallet.last_seen_at && (
        <span className="hidden md:block text-text-dim whitespace-nowrap">
          seen {formatIsoLines(wallet.last_seen_at, timezone).date}
        </span>
      )}
      <div className="flex items-center gap-1 shrink-0">
        <Button size="xs" variant="ghost" onClick={() => onEdit(wallet)}>Edit</Button>
        <Button size="xs" variant="danger" onClick={() => onDelete(wallet)}>âœ•</Button>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Profile card
// ---------------------------------------------------------------------------

interface ProfileCardProps {
  profile: WalletProfile;
  allTags: WalletProfileTag[];
  togglingWalletId?: string | null;
  onEdit: (p: WalletProfile) => void;
  onDelete: (p: WalletProfile) => void;
  onAddWallet: (p: WalletProfile) => void;
  onEditWallet: (w: WalletEntry) => void;
  onDeleteWallet: (w: WalletEntry) => void;
  onToggleWalletTracked: (w: WalletEntry) => void;
  onTagToggle: (profile: WalletProfile, tag: WalletProfileTag, assign: boolean) => void;
}

// Memoized: a card only re-renders when its own profile/tags/toggling flag
// change. With the page's callbacks stabilized via `useCallback`, editing one
// profile no longer re-renders every sibling card.
const ProfileCard = memo(function ProfileCard({
  profile,
  allTags,
  togglingWalletId,
  onEdit,
  onDelete,
  onAddWallet,
  onEditWallet,
  onDeleteWallet,
  onToggleWalletTracked,
  onTagToggle,
}: ProfileCardProps) {
  return (
    <div className="rounded-xl border border-white/8 bg-bg-panel">
      {/* Header */}
      <div className="flex items-center gap-3 border-b border-white/6 px-4 py-3">
        <TypeBadge type={profile.profile_type} />
        <span className="font-semibold text-text text-sm flex-1">{profile.name}</span>
        <span className="text-xs text-text-dim">{profile.wallets.length} wallet{profile.wallets.length !== 1 ? 's' : ''}</span>
        <Button size="xs" variant="ghost" onClick={() => onEdit(profile)}>Edit</Button>
        <Button size="xs" variant="danger" onClick={() => onDelete(profile)}>Delete</Button>
      </div>

      {/* Tags row */}
      <div className="flex items-center gap-1.5 flex-wrap px-4 pt-2.5 pb-1">
        {profile.tags.map((tag) => (
          <TagChip
            key={tag.id}
            tag={tag}
            onRemove={() => onTagToggle(profile, tag, false)}
          />
        ))}
        <TagAssignDropdown
          profile={profile}
          allTags={allTags}
          onToggle={(tag, assign) => onTagToggle(profile, tag, assign)}
        />
      </div>

      {/* Wallets list */}
      <div className="flex flex-col gap-1.5 p-3 pt-2">
        {profile.wallets.length === 0 ? (
          <p className="text-xs text-text-dim px-1">No wallets yet.</p>
        ) : (
          profile.wallets.map((w) => (
            <WalletRow
              key={w.id}
              wallet={w}
              onEdit={onEditWallet}
              onDelete={onDeleteWallet}
              onToggleTracked={onToggleWalletTracked}
              toggling={togglingWalletId === w.id}
            />
          ))
        )}
        <Button
          size="sm"
          variant="ghost"
          className="mt-1 self-start"
          onClick={() => onAddWallet(profile)}
        >
          + Add wallet
        </Button>
      </div>
    </div>
  );
});

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------

export function OtherProfilesPage() {
  const dispatch = useDispatch<AppDispatch>();

  // Profiles live in the shared RTK Query cache (`getProfiles`) â€” the same one
  // the Swing-detection / Sync chart markers read, so an edit here propagates to
  // those overlays without a re-fetch. CRUD still goes through the imperative
  // `services/api` helpers; we patch the cache optimistically with
  // `updateQueryData` (mirrors the `updateSettings`/wallet-holdings pattern).
  const { data: allProfiles = EMPTY_PROFILES, isLoading: loading, error: queryError } =
    useGetProfilesQuery();
  const error = apiErrorMessage(queryError, 'Failed to load profiles') ?? '';

  // The editor manages everyone but "mine" (that lives on the My-wallet page).
  const profiles = useMemo(
    () => allProfiles.filter((p) => p.profile_type !== 'mine'),
    [allProfiles],
  );

  // Tags are their own resource (not part of the shared profiles query), so the
  // page still owns them imperatively.
  const [allTags, setAllTags] = useState<WalletProfileTag[]>([]);

  const [manageTagsOpen, setManageTagsOpen] = useState(false);

  // Modals
  const [profileModal, setProfileModal] = useState<{ open: boolean; profile?: WalletProfile }>({ open: false });
  const [addWalletModal, setAddWalletModal] = useState<{ open: boolean; profileId: string }>({ open: false, profileId: '' });
  const [editWalletModal, setEditWalletModal] = useState<{ open: boolean; wallet: WalletEntry | null }>({ open: false, wallet: null });
  const [deleteProfileModal, setDeleteProfileModal] = useState<{ open: boolean; profile: WalletProfile | null }>({ open: false, profile: null });
  const [deleteWalletModal, setDeleteWalletModal] = useState<{ open: boolean; wallet: WalletEntry | null }>({ open: false, wallet: null });

  // Mutate the shared `getProfiles` cache entry. Returns the RTK patch so the
  // caller can `.undo()` it if the backend write fails.
  const patchProfiles = useCallback(
    (recipe: (draft: WalletProfile[]) => void) =>
      dispatch(sharedApi.util.updateQueryData('getProfiles', undefined, recipe)),
    [dispatch],
  );

  useEffect(() => {
    let cancelled = false;
    fetchTags()
      .then((tags) => { if (!cancelled) setAllTags(tags); })
      .catch(() => { /* tags overview just stays empty on error */ });
    return () => { cancelled = true; };
  }, []);

  // Profile saved (create or edit)
  const handleProfileSaved = useCallback((saved: WalletProfile) => {
    patchProfiles((draft) => {
      const existing = draft.find((p) => p.id === saved.id);
      if (existing) {
        existing.name = saved.name;
        existing.profile_type = saved.profile_type;
      } else {
        draft.push(saved);
      }
    });
  }, [patchProfiles]);

  // Tag CRUD
  const handleTagCreated = useCallback((tag: WalletProfileTag) => setAllTags((prev) => [...prev, tag]), []);

  const handleTagUpdated = useCallback((tag: WalletProfileTag) => {
    setAllTags((prev) => prev.map((t) => (t.id === tag.id ? tag : t)));
    // Reflect update in profiles that have this tag
    patchProfiles((draft) => {
      for (const p of draft) {
        p.tags = p.tags.map((t) => (t.id === tag.id ? tag : t));
      }
    });
  }, [patchProfiles]);

  const handleTagDeleted = useCallback((id: string) => {
    setAllTags((prev) => prev.filter((t) => t.id !== id));
    // Remove from any profiles
    patchProfiles((draft) => {
      for (const p of draft) {
        p.tags = p.tags.filter((t) => t.id !== id);
      }
    });
  }, [patchProfiles]);

  // Per-profile tag toggle (optimistic)
  const handleTagToggle = useCallback(async (profile: WalletProfile, tag: WalletProfileTag, assign: boolean) => {
    const newTags = assign
      ? [...profile.tags, tag]
      : profile.tags.filter((t) => t.id !== tag.id);
    const newTagIds = newTags.map((t) => t.id);

    // Optimistic update
    const patch = patchProfiles((draft) => {
      const target = draft.find((p) => p.id === profile.id);
      if (target) target.tags = newTags;
    });

    try {
      await updateProfileTags(profile.id, newTagIds);
    } catch {
      patch.undo();
    }
  }, [patchProfiles]);

  // Wallet added
  const handleWalletAdded = useCallback((wallet: WalletEntry) => {
    patchProfiles((draft) => {
      const target = draft.find((p) => p.id === wallet.profile_id);
      if (target) target.wallets.push(wallet);
    });
  }, [patchProfiles]);

  // Wallet tracked toggle (optimistic, inline)
  const [togglingWalletId, setTogglingWalletId] = useState<string | null>(null);

  const handleToggleWalletTracked = useCallback(async (wallet: WalletEntry) => {
    if (togglingWalletId) return;
    const next = !wallet.is_tracked;
    setTogglingWalletId(wallet.id);
    // Optimistic
    const patch = patchProfiles((draft) => {
      const target = draft.find((p) => p.id === wallet.profile_id);
      const w = target?.wallets.find((x) => x.id === wallet.id);
      if (w) w.is_tracked = next;
    });
    try {
      await updateWalletEntry(wallet.id, { is_tracked: next, comment: wallet.comment });
    } catch {
      patch.undo();
    } finally {
      setTogglingWalletId(null);
    }
  }, [patchProfiles, togglingWalletId]);

  // Wallet edited
  const handleWalletSaved = useCallback((wallet: WalletEntry) => {
    patchProfiles((draft) => {
      const target = draft.find((p) => p.id === wallet.profile_id);
      if (!target) return;
      const idx = target.wallets.findIndex((w) => w.id === wallet.id);
      if (idx >= 0) target.wallets[idx] = wallet;
    });
  }, [patchProfiles]);

  // Stable open-modal handlers so each memoized ProfileCard keeps the same
  // callback identity across parent renders (no re-render on a sibling edit).
  const handleEditProfile = useCallback((p: WalletProfile) => setProfileModal({ open: true, profile: p }), []);
  const handleDeleteProfile = useCallback((p: WalletProfile) => setDeleteProfileModal({ open: true, profile: p }), []);
  const handleAddWallet = useCallback((p: WalletProfile) => setAddWalletModal({ open: true, profileId: p.id }), []);
  const handleEditWallet = useCallback((w: WalletEntry) => setEditWalletModal({ open: true, wallet: w }), []);
  const handleDeleteWallet = useCallback((w: WalletEntry) => setDeleteWalletModal({ open: true, wallet: w }), []);

  return (
    <div className="mx-auto flex max-w-4xl flex-col gap-6 p-6">
      {/* Page header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-bold text-text">Other profiles</h1>
          <p className="mt-0.5 text-xs text-text-dim">Manage trader, whale, and dev wallet profiles.</p>
        </div>
        <div className="flex items-center gap-2">
          <Button variant="ghost" onClick={() => setManageTagsOpen(true)}>
            Manage tags
          </Button>
          <Button variant="primary" onClick={() => setProfileModal({ open: true })}>
            + New profile
          </Button>
        </div>
      </div>

      {/* Tags overview bar */}
      {allTags.length > 0 && (
        <div className="flex items-center gap-2 flex-wrap rounded-lg border border-white/6 bg-white/2 px-4 py-2.5">
          <span className="text-xs text-text-dim shrink-0">Tags:</span>
          {allTags.map((tag) => (
            <TagChip key={tag.id} tag={tag} />
          ))}
        </div>
      )}

      {/* States */}
      {loading && <p className="text-sm text-text-dim">Loadingâ€¦</p>}
      {error && <InlineAlert variant="error">{error}</InlineAlert>}

      {/* Profile cards */}
      {!loading && profiles.length === 0 && !error && (
        <div className="rounded-xl border border-white/6 bg-white/2 px-6 py-10 text-center text-sm text-text-dim">
          No profiles yet. Create one to start grouping wallets.
        </div>
      )}
      <div className="flex flex-col gap-4">
        {profiles.map((p) => (
          <ProfileCard
            key={p.id}
            profile={p}
            allTags={allTags}
            onEdit={handleEditProfile}
            onDelete={handleDeleteProfile}
            onAddWallet={handleAddWallet}
            onEditWallet={handleEditWallet}
            onDeleteWallet={handleDeleteWallet}
            onToggleWalletTracked={handleToggleWalletTracked}
            togglingWalletId={togglingWalletId}
            onTagToggle={handleTagToggle}
          />
        ))}
      </div>

      {/* Modals */}
      <ManageTagsModal
        open={manageTagsOpen}
        tags={allTags}
        onClose={() => setManageTagsOpen(false)}
        onCreated={handleTagCreated}
        onUpdated={handleTagUpdated}
        onDeleted={handleTagDeleted}
      />

      <ProfileModal
        open={profileModal.open}
        initial={profileModal.profile}
        onClose={() => setProfileModal({ open: false })}
        onSaved={handleProfileSaved}
      />

      <AddWalletModal
        open={addWalletModal.open}
        profileId={addWalletModal.profileId}
        onClose={() => setAddWalletModal({ open: false, profileId: '' })}
        onAdded={handleWalletAdded}
      />

      <EditWalletModal
        open={editWalletModal.open}
        wallet={editWalletModal.wallet}
        onClose={() => setEditWalletModal({ open: false, wallet: null })}
        onSaved={handleWalletSaved}
      />

      <ConfirmDeleteModal
        open={deleteProfileModal.open}
        label={deleteProfileModal.profile?.name ?? ''}
        onClose={() => setDeleteProfileModal({ open: false, profile: null })}
        onConfirm={async () => {
          const id = deleteProfileModal.profile!.id;
          await deleteProfile(id);
          patchProfiles((draft) => {
            const idx = draft.findIndex((p) => p.id === id);
            if (idx >= 0) draft.splice(idx, 1);
          });
        }}
      />

      <ConfirmDeleteModal
        open={deleteWalletModal.open}
        label={deleteWalletModal.wallet ? shortAddr(deleteWalletModal.wallet.address) : ''}
        onClose={() => setDeleteWalletModal({ open: false, wallet: null })}
        onConfirm={async () => {
          const w = deleteWalletModal.wallet!;
          await deleteWalletEntry(w.id);
          patchProfiles((draft) => {
            const target = draft.find((p) => p.id === w.profile_id);
            if (!target) return;
            const idx = target.wallets.findIndex((x) => x.id === w.id);
            if (idx >= 0) target.wallets.splice(idx, 1);
          });
        }}
      />
    </div>
  );
}
