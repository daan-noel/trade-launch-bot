import { useState } from 'react';
import {
  useMetadataTemplatesQuery,
  useCreateMetadataTemplateMutation,
  useUpdateMetadataTemplateMutation,
  useDeleteMetadataTemplateMutation,
} from '@shared/store/endpoints';
import { apiErrorMessage } from '@shared/store/baseApi';
import { AgeCell, Banner, Button, Card, Column, DataTable, Field, IconButton, Input, usePinnedRows } from '@shared/components/ui';
import { fileToBase64 } from '@shared/lib/format';
import type { MetadataTemplate } from '@shared/types';

const emptyForm = {
  templateName: '',
  name: '',
  symbol: '',
  description: '',
  twitter: '',
  telegram: '',
  website: '',
};

export function MetadataTemplatesPage() {
  const { data: templates = [], isFetching } = useMetadataTemplatesQuery();
  const pinning = usePinnedRows('metadata-templates', (t: MetadataTemplate) => t.id, templates);
  const [create, createState] = useCreateMetadataTemplateMutation();
  const [update, updateState] = useUpdateMetadataTemplateMutation();
  const [remove, removeState] = useDeleteMetadataTemplateMutation();

  const [editingId, setEditingId] = useState<string | null>(null);
  const [f, setF] = useState({ ...emptyForm });
  const [imageFile, setImageFile] = useState<File | null>(null);
  const set = (k: keyof typeof f) => (e: React.ChangeEvent<HTMLInputElement>) =>
    setF((p) => ({ ...p, [k]: e.target.value }));

  // A new image is required to author a template, but optional when editing —
  // omitting it keeps the row's already-pinned image and only re-pins the JSON.
  const canSubmit = f.templateName && f.name && f.symbol && (editingId || imageFile);
  const saving = createState.isLoading || updateState.isLoading;

  const resetForm = () => {
    setEditingId(null);
    setF({ ...emptyForm });
    setImageFile(null);
  };

  const onEdit = (t: MetadataTemplate) => {
    setEditingId(t.id);
    setF({
      templateName: t.template_name,
      name: t.name,
      symbol: t.symbol,
      description: t.description ?? '',
      twitter: t.twitter ?? '',
      telegram: t.telegram ?? '',
      website: t.website ?? '',
    });
    setImageFile(null);
    window.scrollTo({ top: 0, behavior: 'smooth' });
  };

  const onSubmit = async () => {
    if (!canSubmit) return;
    try {
      const image = imageFile
        ? {
            image_base64: await fileToBase64(imageFile),
            image_filename: imageFile.name,
            image_content_type: imageFile.type || 'application/octet-stream',
          }
        : undefined;
      const fields = {
        template_name: f.templateName,
        name: f.name,
        symbol: f.symbol,
        description: f.description || undefined,
        twitter: f.twitter || undefined,
        telegram: f.telegram || undefined,
        website: f.website || undefined,
      };
      if (editingId) {
        await update({ id: editingId, body: { ...fields, ...image } }).unwrap();
      } else {
        // create requires an image — canSubmit guarantees `image` here.
        await create({ ...fields, ...image! }).unwrap();
      }
      resetForm();
    } catch {
      /* surfaced below */
    }
  };

  const onDelete = async (t: MetadataTemplate) => {
    if (
      !window.confirm(
        `Delete metadata template "${t.template_name}"? Launch templates using it will be unlinked.`,
      )
    )
      return;
    if (editingId === t.id) resetForm();
    try {
      await remove(t.id).unwrap();
    } catch {
      /* surfaced below */
    }
  };

  const columns: Column<MetadataTemplate>[] = [
    { header: 'Template', render: (t) => t.template_name },
    { header: 'Name', render: (t) => t.name },
    { header: 'Symbol', render: (t) => t.symbol },
    {
      header: 'Image',
      render: (t) =>
        t.image_uri ? (
          <a href={t.image_uri.replace('ipfs://', 'https://ipfs.io/ipfs/')} target="_blank" rel="noreferrer">
            view
          </a>
        ) : (
          <span className="muted">—</span>
        ),
    },
    { header: 'URI', render: (t) => <code className="mono text-xs break-all">{t.uri}</code> },
    { header: 'Age', align: 'right', render: (t) => <AgeCell iso={t.created_at} /> },
    {
      header: '',
      align: 'right',
      render: (t) => (
        <div className="flex justify-end gap-1">
          <IconButton icon="edit" label="Edit" onClick={() => onEdit(t)} />
          <IconButton
            icon="trash"
            label="Delete"
            variant="danger"
            loading={removeState.isLoading && removeState.originalArgs === t.id}
            onClick={() => onDelete(t)}
          />
        </div>
      ),
    },
  ];

  const error =
    apiErrorMessage(createState.error) ??
    apiErrorMessage(updateState.error) ??
    apiErrorMessage(removeState.error);

  return (
    <div className="space-y-4">
      <h1 className="text-lg font-semibold">Metadata Templates</h1>

      <Card title={editingId ? 'Edit metadata template' : 'Author metadata template'}>
        {editingId && (
          <Banner tone="warn" className="mb-3" actions={<Button size="sm" onClick={resetForm}>Cancel</Button>}>
            Editing <strong>{f.templateName}</strong> — re-pins the off-chain JSON. Past launches are
            unaffected.
          </Banner>
        )}

        <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
          <Field label="Template name" htmlFor="m-tn">
            <Input id="m-tn" value={f.templateName} onChange={set('templateName')} placeholder="e.g. dog-coin-v1" />
          </Field>
          <Field label="Name" htmlFor="m-n">
            <Input id="m-n" value={f.name} onChange={set('name')} />
          </Field>
          <Field label="Symbol" htmlFor="m-s">
            <Input id="m-s" value={f.symbol} onChange={set('symbol')} />
          </Field>
          <Field label="Description" htmlFor="m-d" className="md:col-span-3">
            <Input id="m-d" value={f.description} onChange={set('description')} />
          </Field>
          <Field label="Twitter (optional)" htmlFor="m-tw">
            <Input id="m-tw" value={f.twitter} onChange={set('twitter')} />
          </Field>
          <Field label="Telegram (optional)" htmlFor="m-tg">
            <Input id="m-tg" value={f.telegram} onChange={set('telegram')} />
          </Field>
          <Field label="Website (optional)" htmlFor="m-w">
            <Input id="m-w" value={f.website} onChange={set('website')} />
          </Field>
          <Field
            label={editingId ? 'Image (optional — keeps current if blank)' : 'Image'}
            htmlFor="m-img"
            className="md:col-span-2"
          >
            <Input
              id="m-img"
              type="file"
              accept="image/*"
              onChange={(e) => setImageFile(e.target.files?.[0] ?? null)}
            />
          </Field>
          <div className="flex items-end">
            <Button variant="primary" loading={saving} disabled={!canSubmit} onClick={onSubmit}>
              {saving ? 'Pinning to IPFS…' : editingId ? 'Save changes' : 'Create template'}
            </Button>
          </div>
        </div>
        <p className="mt-2 text-xs muted">
          Pins the image, then the standard off-chain JSON, to IPFS via Pinata (requires{' '}
          <code>PINATA_JWT</code> on the live box). The resulting <code>uri</code> is the token
          identity a launch template references via <code>metadata_template_id</code> — the single
          source of truth.
        </p>
        {error && <Banner tone="bad" className="mt-3">{error}</Banner>}
      </Card>

      <Card title={`Templates (${templates.length})`}>
        <DataTable
          columns={columns}
          rows={templates}
          rowKey={(t) => t.id}
          loading={isFetching}
          empty="No metadata templates yet — author one above."
          pinning={pinning}
        />
      </Card>
    </div>
  );
}
