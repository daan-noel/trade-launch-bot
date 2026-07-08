import { useEffect, useState } from 'react';
import {
  api,
  BuyVariant,
  formatAge,
  Launchpad,
  LaunchTemplate,
  LegStructureRecipe,
  MetadataTemplate,
  NewLaunchTemplateInput,
  QuoteAsset,
} from './api';
import { Column, DataTable } from './components/DataTable';

const VARIANTS = [
  'pumpfun.create_v2',
  'pumpfun.create_v2_devbuy',
  'pumpfun.create_v1',
  'pumpfun.create_v1_devbuy',
] as const;

const BUY_VARIANTS: BuyVariant[] = ['buy', 'buy_exact_sol_in', 'buy_v2', 'buy_exact_quote_in'];

interface LegRow {
  variant: BuyVariant;
  slippage_bps_min: string;
  slippage_bps_max: string;
  cu_limit_min: string;
  cu_limit_max: string;
  cu_price_min: string;
  cu_price_max: string;
  // Human quote units (converted against the form's selected quote asset).
  tip_quote_min: string;
  tip_quote_max: string;
}

function emptyLegRow(): LegRow {
  return {
    variant: 'buy_exact_sol_in',
    slippage_bps_min: '',
    slippage_bps_max: '',
    cu_limit_min: '',
    cu_limit_max: '',
    cu_price_min: '',
    cu_price_max: '',
    tip_quote_min: '',
    tip_quote_max: '',
  };
}

// Amounts in `params` are quote base units (lamports for SOL) — the form
// works in human units against whichever quote asset is selected, converting
// at the decimals boundary so operators never hand-compute lamports.
function toBaseUnits(human: string, decimals: number): number | undefined {
  if (!human.trim()) return undefined;
  const n = Number(human);
  if (!Number.isFinite(n)) return undefined;
  return Math.round(n * 10 ** decimals);
}

function toHumanUnits(base: number | null | undefined, decimals: number): string {
  if (base == null) return '';
  return String(base / 10 ** decimals);
}

function toInt(value: string): number | undefined {
  if (!value.trim()) return undefined;
  const n = Number(value);
  return Number.isFinite(n) ? Math.round(n) : undefined;
}

function legRowToRecipe(row: LegRow, decimals: number): LegStructureRecipe {
  return {
    variant: row.variant,
    slippage_bps_min: toInt(row.slippage_bps_min),
    slippage_bps_max: toInt(row.slippage_bps_max),
    cu_limit_min: toInt(row.cu_limit_min),
    cu_limit_max: toInt(row.cu_limit_max),
    cu_price_min: toInt(row.cu_price_min),
    cu_price_max: toInt(row.cu_price_max),
    tip_quote_min: toBaseUnits(row.tip_quote_min, decimals),
    tip_quote_max: toBaseUnits(row.tip_quote_max, decimals),
  };
}

function recipeToLegRow(recipe: LegStructureRecipe, decimals: number): LegRow {
  return {
    variant: recipe.variant,
    slippage_bps_min: recipe.slippage_bps_min?.toString() ?? '',
    slippage_bps_max: recipe.slippage_bps_max?.toString() ?? '',
    cu_limit_min: recipe.cu_limit_min?.toString() ?? '',
    cu_limit_max: recipe.cu_limit_max?.toString() ?? '',
    cu_price_min: recipe.cu_price_min?.toString() ?? '',
    cu_price_max: recipe.cu_price_max?.toString() ?? '',
    tip_quote_min: toHumanUnits(recipe.tip_quote_min, decimals),
    tip_quote_max: toHumanUnits(recipe.tip_quote_max, decimals),
  };
}

export default function LaunchTemplates({ onChange }: { onChange?: () => void }) {
  const [templates, setTemplates] = useState<LaunchTemplate[]>([]);
  const [launchpads, setLaunchpads] = useState<Launchpad[]>([]);
  const [quoteAssets, setQuoteAssets] = useState<QuoteAsset[]>([]);
  const [metadataTemplates, setMetadataTemplates] = useState<MetadataTemplate[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const [editingId, setEditingId] = useState<string | null>(null);
  const [templateName, setTemplateName] = useState('');
  const [launchpadId, setLaunchpadId] = useState('');
  const [quoteAssetId, setQuoteAssetId] = useState('');
  const [variant, setVariant] = useState<string>(VARIANTS[0]);
  // The metadata_templates row this template launches with — the single source
  // of truth for token identity (name/symbol/uri). Stored as an FK, not inlined.
  const [metadataTemplateId, setMetadataTemplateId] = useState('');
  const [devBuyQuote, setDevBuyQuote] = useState('');
  const [slippageBps, setSlippageBps] = useState('');
  const [isMayhemMode, setIsMayhemMode] = useState(false);
  const [cashbackEnabled, setCashbackEnabled] = useState(false);
  const [bundleLegCount, setBundleLegCount] = useState('');
  const [bundleQuotePerLeg, setBundleQuotePerLeg] = useState('');
  const [bundleTipQuote, setBundleTipQuote] = useState('');
  const [legRows, setLegRows] = useState<LegRow[]>([]);

  const selectedQuoteAsset = quoteAssets.find((q) => String(q.id) === quoteAssetId);
  const decimals = selectedQuoteAsset?.decimals ?? 9;
  const quoteSymbol = selectedQuoteAsset?.symbol ?? 'quote';

  const load = async () => {
    setLoading(true);
    setError(null);
    try {
      const [t, lp, qa, mt] = await Promise.all([
        api.templates(),
        api.launchpads(),
        api.quoteAssets(),
        api.metadataTemplates(),
      ]);
      setTemplates(t);
      setLaunchpads(lp);
      setQuoteAssets(qa);
      setMetadataTemplates(mt);
      if (!launchpadId && lp[0]) setLaunchpadId(String(lp[0].id));
      if (!quoteAssetId) {
        const native = qa.find((q) => q.is_native) ?? qa[0];
        if (native) setQuoteAssetId(String(native.id));
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const resetForm = () => {
    setEditingId(null);
    setTemplateName('');
    setVariant(VARIANTS[0]);
    setMetadataTemplateId('');
    setDevBuyQuote('');
    setSlippageBps('');
    setIsMayhemMode(false);
    setCashbackEnabled(false);
    setBundleLegCount('');
    setBundleQuotePerLeg('');
    setBundleTipQuote('');
    setLegRows([]);
    if (launchpads[0]) setLaunchpadId(String(launchpads[0].id));
    const native = quoteAssets.find((q) => q.is_native) ?? quoteAssets[0];
    if (native) setQuoteAssetId(String(native.id));
  };

  const onEdit = (t: LaunchTemplate) => {
    const legDecimals = quoteAssets.find((q) => q.id === t.quote_asset_id)?.decimals ?? decimals;
    setEditingId(t.id);
    setTemplateName(t.template_name);
    setLaunchpadId(String(t.launchpad_id));
    setQuoteAssetId(String(t.quote_asset_id));
    setVariant(t.variant);
    setMetadataTemplateId(t.metadata_template_id ?? '');
    setDevBuyQuote(toHumanUnits(t.params.dev_buy_quote, legDecimals));
    setSlippageBps(t.params.slippage_bps?.toString() ?? '');
    setIsMayhemMode(t.params.is_mayhem_mode ?? false);
    setCashbackEnabled(t.params.cashback_enabled ?? false);
    setBundleLegCount(t.params.bundle_leg_count?.toString() ?? '');
    setBundleQuotePerLeg(toHumanUnits(t.params.bundle_quote_per_leg, legDecimals));
    setBundleTipQuote(toHumanUnits(t.params.bundle_tip_quote, legDecimals));
    setLegRows((t.params.leg_structures ?? []).map((r) => recipeToLegRow(r, legDecimals)));
  };

  const addLegRow = () => setLegRows((rows) => [...rows, emptyLegRow()]);
  const removeLegRow = (i: number) => setLegRows((rows) => rows.filter((_, idx) => idx !== i));
  const updateLegRow = (i: number, patch: Partial<LegRow>) =>
    setLegRows((rows) => rows.map((r, idx) => (idx === i ? { ...r, ...patch } : r)));

  const columns: Column<LaunchTemplate>[] = [
    { header: 'Template', render: (t) => t.template_name },
    {
      header: 'Metadata',
      render: (t) => {
        const m = metadataTemplates.find((x) => x.id === t.metadata_template_id);
        return m ? `${m.name} / ${m.symbol}` : <span className="muted">— unlinked —</span>;
      },
    },
    { header: 'Variant', render: (t) => t.variant },
    {
      header: 'Launchpad',
      render: (t) => launchpads.find((lp) => lp.id === t.launchpad_id)?.display_name ?? t.launchpad_id,
    },
    {
      header: 'Quote',
      render: (t) => quoteAssets.find((qa) => qa.id === t.quote_asset_id)?.symbol ?? t.quote_asset_id,
    },
    { header: 'Created', render: (t) => formatAge(t.created_at) },
    { header: 'Updated', render: (t) => formatAge(t.updated_at) },
    {
      header: '',
      render: (t) => (
        <button type="button" onClick={() => onEdit(t)}>
          Edit
        </button>
      ),
    },
  ];

  const canSubmit =
    templateName && launchpadId && quoteAssetId && variant && metadataTemplateId;

  const onSubmit = async () => {
    if (!canSubmit) return;
    setSaving(true);
    setError(null);
    try {
      const input: NewLaunchTemplateInput = {
        template_name: templateName,
        launchpad_id: Number(launchpadId),
        variant,
        quote_asset_id: Number(quoteAssetId),
        metadata_template_id: metadataTemplateId,
        params: {
          dev_buy_quote: toBaseUnits(devBuyQuote, decimals),
          slippage_bps: toInt(slippageBps),
          is_mayhem_mode: isMayhemMode,
          cashback_enabled: cashbackEnabled,
          bundle_leg_count: toInt(bundleLegCount),
          bundle_quote_per_leg: toBaseUnits(bundleQuotePerLeg, decimals),
          bundle_tip_quote: toBaseUnits(bundleTipQuote, decimals),
          leg_structures: legRows.length ? legRows.map((r) => legRowToRecipe(r, decimals)) : undefined,
        },
      };
      if (editingId) {
        await api.updateLaunchTemplate(editingId, input);
      } else {
        await api.createLaunchTemplate(input);
      }
      resetForm();
      await load();
      onChange?.();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div>
      <div className="card">
        <h2>{editingId ? 'Edit launch template' : 'Author launch template'}</h2>
        {editingId && (
          <p className="banner-warning">
            Editing: <strong>{templateName}</strong> — editing does not retroactively change past
            launches.{' '}
            <button type="button" onClick={resetForm}>
              Cancel
            </button>
          </p>
        )}
        <div className="row">
          <div className="field">
            <label htmlFor="lt-template-name">Template name</label>
            <input
              id="lt-template-name"
              type="text"
              value={templateName}
              onChange={(e) => setTemplateName(e.target.value)}
              placeholder="e.g. standard-v2-bundle"
            />
          </div>
          <div className="field">
            <label htmlFor="lt-launchpad">Launchpad</label>
            <select id="lt-launchpad" value={launchpadId} onChange={(e) => setLaunchpadId(e.target.value)}>
              {launchpads.length === 0 && <option value="">No launchpads</option>}
              {launchpads.map((lp) => (
                <option key={lp.id} value={lp.id}>
                  {lp.display_name}
                </option>
              ))}
            </select>
          </div>
          <div className="field">
            <label htmlFor="lt-quote-asset">Quote asset</label>
            <select id="lt-quote-asset" value={quoteAssetId} onChange={(e) => setQuoteAssetId(e.target.value)}>
              {quoteAssets.length === 0 && <option value="">No quote assets</option>}
              {quoteAssets.map((qa) => (
                <option key={qa.id} value={qa.id}>
                  {qa.symbol}
                </option>
              ))}
            </select>
          </div>
          <div className="field">
            <label htmlFor="lt-variant">Variant</label>
            <select id="lt-variant" value={variant} onChange={(e) => setVariant(e.target.value)}>
              {VARIANTS.map((v) => (
                <option key={v} value={v}>
                  {v}
                </option>
              ))}
            </select>
          </div>
        </div>

        <div className="row">
          <div className="field" style={{ flex: 1 }}>
            <label htmlFor="lt-metadata-template">Metadata template (name/symbol/URI)</label>
            <select
              id="lt-metadata-template"
              value={metadataTemplateId}
              onChange={(e) => setMetadataTemplateId(e.target.value)}
            >
              <option value="">— select a metadata template —</option>
              {metadataTemplates.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.template_name} ({m.name} / {m.symbol})
                </option>
              ))}
            </select>
            {metadataTemplates.length === 0 && (
              <p className="muted">No metadata templates yet — create one in the Metadata tab first.</p>
            )}
          </div>
        </div>

        <div className="row">
          <div className="field">
            <label htmlFor="lt-dev-buy">{`Dev buy (${quoteSymbol})`}</label>
            <input
              id="lt-dev-buy"
              type="text"
              value={devBuyQuote}
              onChange={(e) => setDevBuyQuote(e.target.value)}
              placeholder="0.05"
            />
          </div>
          <div className="field">
            <label htmlFor="lt-slippage">Slippage (bps)</label>
            <input
              id="lt-slippage"
              type="number"
              value={slippageBps}
              onChange={(e) => setSlippageBps(e.target.value)}
            />
          </div>
          <div className="field">
            <label htmlFor="lt-mayhem">Mayhem mode</label>
            <input
              id="lt-mayhem"
              type="checkbox"
              checked={isMayhemMode}
              onChange={(e) => setIsMayhemMode(e.target.checked)}
            />
          </div>
          <div className="field">
            <label htmlFor="lt-cashback">Cashback enabled</label>
            <input
              id="lt-cashback"
              type="checkbox"
              checked={cashbackEnabled}
              onChange={(e) => setCashbackEnabled(e.target.checked)}
            />
          </div>
        </div>

        <div className="row">
          <div className="field">
            <label htmlFor="lt-leg-count">Default bundle leg count</label>
            <input
              id="lt-leg-count"
              type="number"
              min={0}
              value={bundleLegCount}
              onChange={(e) => setBundleLegCount(e.target.value)}
            />
          </div>
          <div className="field">
            <label htmlFor="lt-quote-per-leg">{`Quote per leg (${quoteSymbol})`}</label>
            <input
              id="lt-quote-per-leg"
              type="text"
              value={bundleQuotePerLeg}
              onChange={(e) => setBundleQuotePerLeg(e.target.value)}
              placeholder="0.02"
            />
          </div>
          <div className="field">
            <label htmlFor="lt-bundle-tip">{`Bundle tip (${quoteSymbol})`}</label>
            <input
              id="lt-bundle-tip"
              type="text"
              value={bundleTipQuote}
              onChange={(e) => setBundleTipQuote(e.target.value)}
              placeholder="0.001"
            />
          </div>
        </div>

        <h3>Leg structures</h3>
        <p className="muted">
          A field left blank falls back to the composer's default range: slippage 300-800 bps,
          cu_limit 120,000-180,000, cu_price 150,000-400,000, tip 10,000-100,000 lamports.
        </p>
        {legRows.map((row, i) => (
          <div className="row" key={i}>
            <div className="field">
              <label htmlFor={`lt-leg-${i}-variant`}>Buy variant</label>
              <select
                id={`lt-leg-${i}-variant`}
                value={row.variant}
                onChange={(e) => updateLegRow(i, { variant: e.target.value as BuyVariant })}
              >
                {BUY_VARIANTS.map((v) => (
                  <option key={v} value={v}>
                    {v}
                  </option>
                ))}
              </select>
            </div>
            <div className="field">
              <label>Slippage bps min/max</label>
              <input
                type="number"
                value={row.slippage_bps_min}
                onChange={(e) => updateLegRow(i, { slippage_bps_min: e.target.value })}
                placeholder="300"
              />
              <input
                type="number"
                value={row.slippage_bps_max}
                onChange={(e) => updateLegRow(i, { slippage_bps_max: e.target.value })}
                placeholder="800"
              />
            </div>
            <div className="field">
              <label>CU limit min/max</label>
              <input
                type="number"
                value={row.cu_limit_min}
                onChange={(e) => updateLegRow(i, { cu_limit_min: e.target.value })}
                placeholder="120000"
              />
              <input
                type="number"
                value={row.cu_limit_max}
                onChange={(e) => updateLegRow(i, { cu_limit_max: e.target.value })}
                placeholder="180000"
              />
            </div>
            <div className="field">
              <label>CU price min/max</label>
              <input
                type="number"
                value={row.cu_price_min}
                onChange={(e) => updateLegRow(i, { cu_price_min: e.target.value })}
                placeholder="150000"
              />
              <input
                type="number"
                value={row.cu_price_max}
                onChange={(e) => updateLegRow(i, { cu_price_max: e.target.value })}
                placeholder="400000"
              />
            </div>
            <div className="field">
              <label>{`Tip ${quoteSymbol} min/max`}</label>
              <input
                type="text"
                value={row.tip_quote_min}
                onChange={(e) => updateLegRow(i, { tip_quote_min: e.target.value })}
                placeholder="0.00001"
              />
              <input
                type="text"
                value={row.tip_quote_max}
                onChange={(e) => updateLegRow(i, { tip_quote_max: e.target.value })}
                placeholder="0.0001"
              />
            </div>
            <button type="button" onClick={() => removeLegRow(i)}>
              Remove
            </button>
          </div>
        ))}
        <div className="row">
          <button type="button" onClick={addLegRow}>
            Add leg structure
          </button>
        </div>

        <div className="row">
          <button type="button" className="primary" disabled={saving || !canSubmit} onClick={onSubmit}>
            {saving ? 'Saving…' : editingId ? 'Save changes' : 'Create template'}
          </button>
        </div>
        {error && <p className="error">{error}</p>}
      </div>

      <div className="card">
        <h2>Templates ({templates.length})</h2>
        <DataTable
          columns={columns}
          rows={templates}
          rowKey={(t) => t.id}
          loading={loading}
          empty="No launch templates yet — author one above."
        />
      </div>
    </div>
  );
}
