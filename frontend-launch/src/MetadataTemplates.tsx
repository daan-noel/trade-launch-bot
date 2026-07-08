import { useState } from 'react';
import { api, fileToBase64, formatAge, MetadataTemplate } from './api';
import { useResource } from './components/useResource';
import { Column, DataTable } from './components/DataTable';
import { Field } from './components/Field';

export default function MetadataTemplates() {
  const {
    data: templates = [],
    loading,
    error,
    reload,
    setError,
  } = useResource(() => api.metadataTemplates(), []);

  const [templateName, setTemplateName] = useState('');
  const [name, setName] = useState('');
  const [symbol, setSymbol] = useState('');
  const [description, setDescription] = useState('');
  const [twitter, setTwitter] = useState('');
  const [telegram, setTelegram] = useState('');
  const [website, setWebsite] = useState('');
  const [imageFile, setImageFile] = useState<File | null>(null);
  const [creating, setCreating] = useState(false);

  const onCreate = async () => {
    if (!templateName || !name || !symbol || !imageFile) return;
    setCreating(true);
    setError(null);
    try {
      const image_base64 = await fileToBase64(imageFile);
      await api.createMetadataTemplate({
        template_name: templateName,
        name,
        symbol,
        description: description || undefined,
        twitter: twitter || undefined,
        telegram: telegram || undefined,
        website: website || undefined,
        image_base64,
        image_filename: imageFile.name,
        image_content_type: imageFile.type || 'application/octet-stream',
      });
      setTemplateName('');
      setName('');
      setSymbol('');
      setDescription('');
      setTwitter('');
      setTelegram('');
      setWebsite('');
      setImageFile(null);
      await reload();
    } catch (e) {
      setError(String(e));
    } finally {
      setCreating(false);
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
          <a
            href={t.image_uri.replace('ipfs://', 'https://ipfs.io/ipfs/')}
            target="_blank"
            rel="noreferrer"
          >
            view
          </a>
        ) : (
          <span className="muted">—</span>
        ),
    },
    { header: 'URI', render: (t) => <code>{t.uri}</code> },
    { header: 'Age', render: (t) => formatAge(t.created_at) },
  ];

  return (
    <div>
      <div className="card">
        <h2>Author metadata template</h2>
        <div className="row">
          <Field label="Template name" htmlFor="meta-template-name">
            <input
              id="meta-template-name"
              type="text"
              value={templateName}
              onChange={(e) => setTemplateName(e.target.value)}
              placeholder="e.g. dog-coin-v1"
            />
          </Field>
          <Field label="Name" htmlFor="meta-name">
            <input id="meta-name" type="text" value={name} onChange={(e) => setName(e.target.value)} />
          </Field>
          <Field label="Symbol" htmlFor="meta-symbol">
            <input id="meta-symbol" type="text" value={symbol} onChange={(e) => setSymbol(e.target.value)} />
          </Field>
        </div>
        <div className="row">
          <Field label="Description" htmlFor="meta-description" style={{ flex: 1 }}>
            <input
              id="meta-description"
              type="text"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
            />
          </Field>
        </div>
        <div className="row">
          <Field label="Twitter (optional)" htmlFor="meta-twitter">
            <input id="meta-twitter" type="text" value={twitter} onChange={(e) => setTwitter(e.target.value)} />
          </Field>
          <Field label="Telegram (optional)" htmlFor="meta-telegram">
            <input
              id="meta-telegram"
              type="text"
              value={telegram}
              onChange={(e) => setTelegram(e.target.value)}
            />
          </Field>
          <Field label="Website (optional)" htmlFor="meta-website">
            <input id="meta-website" type="text" value={website} onChange={(e) => setWebsite(e.target.value)} />
          </Field>
        </div>
        <div className="row">
          <Field label="Image" htmlFor="meta-image">
            <input
              id="meta-image"
              type="file"
              accept="image/*"
              onChange={(e) => setImageFile(e.target.files?.[0] ?? null)}
            />
          </Field>
          <button
            type="button"
            className="primary"
            disabled={creating || !templateName || !name || !symbol || !imageFile}
            onClick={onCreate}
          >
            {creating ? 'Pinning to IPFS…' : 'Create template'}
          </button>
        </div>
        <p className="muted">
          Pins the image, then the standard off-chain JSON, to IPFS via Pinata (requires{' '}
          <code>PINATA_JWT</code> configured on the live box). The resulting <code>uri</code> is the
          token identity a launch template references via <code>metadata_template_id</code> (the
          single source of truth) and the launcher resolves at create time.
        </p>
        {error && <p className="error">{error}</p>}
      </div>

      <div className="card">
        <h2>Templates ({templates.length})</h2>
        <DataTable
          columns={columns}
          rows={templates}
          rowKey={(t) => t.id}
          loading={loading}
          empty="No metadata templates yet — author one above."
        />
      </div>
    </div>
  );
}
