import { useEffect, useState } from 'react';
import { api, fileToBase64, formatAge, MetadataTemplate } from './api';

export default function MetadataTemplates() {
  const [templates, setTemplates] = useState<MetadataTemplate[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [templateName, setTemplateName] = useState('');
  const [name, setName] = useState('');
  const [symbol, setSymbol] = useState('');
  const [description, setDescription] = useState('');
  const [twitter, setTwitter] = useState('');
  const [telegram, setTelegram] = useState('');
  const [website, setWebsite] = useState('');
  const [imageFile, setImageFile] = useState<File | null>(null);
  const [creating, setCreating] = useState(false);

  const load = async () => {
    setLoading(true);
    setError(null);
    try {
      setTemplates(await api.metadataTemplates());
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    load();
  }, []);

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
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setCreating(false);
    }
  };

  return (
    <div>
      <div className="card">
        <h2>Author metadata template</h2>
        <div className="row">
          <div className="field">
            <label htmlFor="meta-template-name">Template name</label>
            <input
              id="meta-template-name"
              type="text"
              value={templateName}
              onChange={(e) => setTemplateName(e.target.value)}
              placeholder="e.g. dog-coin-v1"
            />
          </div>
          <div className="field">
            <label htmlFor="meta-name">Name</label>
            <input id="meta-name" type="text" value={name} onChange={(e) => setName(e.target.value)} />
          </div>
          <div className="field">
            <label htmlFor="meta-symbol">Symbol</label>
            <input id="meta-symbol" type="text" value={symbol} onChange={(e) => setSymbol(e.target.value)} />
          </div>
        </div>
        <div className="row">
          <div className="field" style={{ flex: 1 }}>
            <label htmlFor="meta-description">Description</label>
            <input
              id="meta-description"
              type="text"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
            />
          </div>
        </div>
        <div className="row">
          <div className="field">
            <label htmlFor="meta-twitter">Twitter (optional)</label>
            <input id="meta-twitter" type="text" value={twitter} onChange={(e) => setTwitter(e.target.value)} />
          </div>
          <div className="field">
            <label htmlFor="meta-telegram">Telegram (optional)</label>
            <input
              id="meta-telegram"
              type="text"
              value={telegram}
              onChange={(e) => setTelegram(e.target.value)}
            />
          </div>
          <div className="field">
            <label htmlFor="meta-website">Website (optional)</label>
            <input id="meta-website" type="text" value={website} onChange={(e) => setWebsite(e.target.value)} />
          </div>
        </div>
        <div className="row">
          <div className="field">
            <label htmlFor="meta-image">Image</label>
            <input
              id="meta-image"
              type="file"
              accept="image/*"
              onChange={(e) => setImageFile(e.target.files?.[0] ?? null)}
            />
          </div>
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
          <code>PINATA_JWT</code> configured on the live box). The resulting <code>uri</code> is what
          a launch template's <code>uri</code> field / the Launch Console's metadata-URI override
          consumes.
        </p>
        {error && <p className="error">{error}</p>}
      </div>

      <div className="card">
        <h2>Templates ({templates.length})</h2>
        {loading ? (
          <p className="muted">Loading…</p>
        ) : (
          <table>
            <thead>
              <tr>
                <th>Template</th>
                <th>Name</th>
                <th>Symbol</th>
                <th>Image</th>
                <th>URI</th>
                <th>Age</th>
              </tr>
            </thead>
            <tbody>
              {templates.map((t) => (
                <tr key={t.id}>
                  <td>{t.template_name}</td>
                  <td>{t.name}</td>
                  <td>{t.symbol}</td>
                  <td>
                    <a href={t.image_uri.replace('ipfs://', 'https://ipfs.io/ipfs/')} target="_blank" rel="noreferrer">
                      view
                    </a>
                  </td>
                  <td>
                    <code>{t.uri}</code>
                  </td>
                  <td>{formatAge(t.created_at)}</td>
                </tr>
              ))}
              {templates.length === 0 && (
                <tr>
                  <td colSpan={6} className="muted">
                    No metadata templates yet — author one above.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
