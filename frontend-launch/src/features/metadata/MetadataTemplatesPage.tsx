import { useState } from 'react';
import {
  useMetadataTemplatesQuery,
  useCreateMetadataTemplateMutation,
} from '@shared/store/endpoints';
import { apiErrorMessage } from '@shared/store/baseApi';
import { Banner, Button, Card, Column, DataTable, Field, Input } from '@shared/components/ui';
import { fileToBase64, formatAge } from '@shared/lib/format';
import type { MetadataTemplate } from '@shared/types';

export function MetadataTemplatesPage() {
  const { data: templates = [], isFetching } = useMetadataTemplatesQuery();
  const [create, { isLoading, error }] = useCreateMetadataTemplateMutation();

  const [f, setF] = useState({
    templateName: '',
    name: '',
    symbol: '',
    description: '',
    twitter: '',
    telegram: '',
    website: '',
  });
  const [imageFile, setImageFile] = useState<File | null>(null);
  const set = (k: keyof typeof f) => (e: React.ChangeEvent<HTMLInputElement>) =>
    setF((p) => ({ ...p, [k]: e.target.value }));

  const canSubmit = f.templateName && f.name && f.symbol && imageFile;

  const onCreate = async () => {
    if (!canSubmit || !imageFile) return;
    try {
      const image_base64 = await fileToBase64(imageFile);
      await create({
        template_name: f.templateName,
        name: f.name,
        symbol: f.symbol,
        description: f.description || undefined,
        twitter: f.twitter || undefined,
        telegram: f.telegram || undefined,
        website: f.website || undefined,
        image_base64,
        image_filename: imageFile.name,
        image_content_type: imageFile.type || 'application/octet-stream',
      }).unwrap();
      setF({ templateName: '', name: '', symbol: '', description: '', twitter: '', telegram: '', website: '' });
      setImageFile(null);
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
    { header: 'Age', align: 'right', render: (t) => formatAge(t.created_at) },
  ];

  return (
    <div className="space-y-4">
      <h1 className="text-lg font-semibold">Metadata Templates</h1>

      <Card title="Author metadata template">
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
          <Field label="Image" htmlFor="m-img" className="md:col-span-2">
            <Input
              id="m-img"
              type="file"
              accept="image/*"
              onChange={(e) => setImageFile(e.target.files?.[0] ?? null)}
            />
          </Field>
          <div className="flex items-end">
            <Button variant="primary" loading={isLoading} disabled={!canSubmit} onClick={onCreate}>
              {isLoading ? 'Pinning to IPFS…' : 'Create template'}
            </Button>
          </div>
        </div>
        <p className="mt-2 text-xs muted">
          Pins the image, then the standard off-chain JSON, to IPFS via Pinata (requires{' '}
          <code>PINATA_JWT</code> on the live box). The resulting <code>uri</code> is the token
          identity a launch template references via <code>metadata_template_id</code> — the single
          source of truth.
        </p>
        {error && <Banner tone="bad" className="mt-3">{apiErrorMessage(error)}</Banner>}
      </Card>

      <Card title={`Templates (${templates.length})`}>
        <DataTable
          columns={columns}
          rows={templates}
          rowKey={(t) => t.id}
          loading={isFetching}
          empty="No metadata templates yet — author one above."
        />
      </Card>
    </div>
  );
}
