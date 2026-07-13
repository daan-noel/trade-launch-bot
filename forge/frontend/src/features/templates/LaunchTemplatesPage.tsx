import { useEffect, useMemo, useState } from 'react';
import {
  useTemplatesQuery,
  useLaunchpadsQuery,
  useQuoteAssetsQuery,
  useMetadataTemplatesQuery,
  useCreateTemplateMutation,
  useUpdateTemplateMutation,
  useDeleteTemplateMutation,
} from '@shared/store/endpoints';
import { apiErrorMessage } from '@shared/store/baseApi';
import {
  AgeCell,
  Banner,
  Button,
  Card,
  Checkbox,
  Column,
  DataTable,
  Field,
  Input,
  Select,
} from '@shared/components/ui';
import { IxLayoutEditor } from '@shared/components/IxLayoutEditor';
import type { BuyVariant, DecoStep, LaunchTemplate, NewLaunchTemplateInput } from '@shared/types';
import {
  BUY_VARIANTS,
  VARIANTS,
  emptyLegRow,
  legRowToRecipe,
  recipeToLegRow,
  toBaseUnits,
  toHumanUnits,
  toInt,
  type LegRow,
} from './legForm';

export function LaunchTemplatesPage() {
  const { data: templates = [], isFetching } = useTemplatesQuery();
  const { data: launchpads = [] } = useLaunchpadsQuery();
  const { data: quoteAssets = [] } = useQuoteAssetsQuery();
  const { data: metadataTemplates = [] } = useMetadataTemplatesQuery();
  const [create, createState] = useCreateTemplateMutation();
  const [update, updateState] = useUpdateTemplateMutation();
  const [remove, removeState] = useDeleteTemplateMutation();

  const [editingId, setEditingId] = useState<string | null>(null);
  const [templateName, setTemplateName] = useState('');
  const [launchpadId, setLaunchpadId] = useState('');
  const [quoteAssetId, setQuoteAssetId] = useState('');
  const [variant, setVariant] = useState<string>(VARIANTS[0]);
  const [metadataTemplateId, setMetadataTemplateId] = useState('');
  const [devBuyQuote, setDevBuyQuote] = useState('');
  const [slippageBps, setSlippageBps] = useState('');
  const [isMayhemMode, setIsMayhemMode] = useState(false);
  const [cashbackEnabled, setCashbackEnabled] = useState(false);
  const [bundleLegCount, setBundleLegCount] = useState('');
  const [bundleQuotePerLeg, setBundleQuotePerLeg] = useState('');
  const [bundleTipQuote, setBundleTipQuote] = useState('');
  const [legRows, setLegRows] = useState<LegRow[]>([]);
  const [createLayout, setCreateLayout] = useState<DecoStep[] | undefined>(undefined);

  const selectedQuoteAsset = quoteAssets.find((qq) => String(qq.id) === quoteAssetId);
  const decimals = selectedQuoteAsset?.decimals ?? 9;
  const quoteSymbol = selectedQuoteAsset?.symbol ?? 'quote';

  // Seed default launchpad / native quote once the dimensions arrive.
  useEffect(() => {
    if (!launchpadId && launchpads[0]) setLaunchpadId(String(launchpads[0].id));
    if (!quoteAssetId) {
      const native = quoteAssets.find((qq) => qq.is_native) ?? quoteAssets[0];
      if (native) setQuoteAssetId(String(native.id));
    }
  }, [launchpads, quoteAssets, launchpadId, quoteAssetId]);

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
    setCreateLayout(undefined);
    if (launchpads[0]) setLaunchpadId(String(launchpads[0].id));
    const native = quoteAssets.find((qq) => qq.is_native) ?? quoteAssets[0];
    if (native) setQuoteAssetId(String(native.id));
  };

  const onEdit = (t: LaunchTemplate) => {
    const legDecimals = quoteAssets.find((qq) => qq.id === t.quote_asset_id)?.decimals ?? decimals;
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
    setCreateLayout(t.params.create_layout);
    window.scrollTo({ top: 0, behavior: 'smooth' });
  };

  const onDelete = async (t: LaunchTemplate) => {
    if (!window.confirm(`Delete launch template "${t.template_name}"? Past launches are unaffected.`)) return;
    if (editingId === t.id) resetForm();
    try {
      await remove(t.id).unwrap();
    } catch {
      /* surfaced below */
    }
  };

  const addLegRow = () => setLegRows((r) => [...r, emptyLegRow()]);
  const removeLegRow = (i: number) => setLegRows((r) => r.filter((_, idx) => idx !== i));
  const updateLegRow = (i: number, patch: Partial<LegRow>) =>
    setLegRows((r) => r.map((row, idx) => (idx === i ? { ...row, ...patch } : row)));

  // Mirror the backend `UniformLayout` audit rule: ≥3 legs sharing an identical
  // authored layout is a fingerprint tell (the gate rejects it without allow_fingerprint).
  const uniformLayoutWarning = useMemo(() => {
    const counts = new Map<string, number>();
    for (const r of legRows) {
      if (r.layout) counts.set(r.layout.join(','), (counts.get(r.layout.join(',')) ?? 0) + 1);
    }
    return [...counts.values()].some((n) => n >= 3);
  }, [legRows]);

  const canSubmit = templateName && launchpadId && quoteAssetId && variant && metadataTemplateId;

  const onSubmit = async () => {
    if (!canSubmit) return;
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
        create_layout: createLayout,
      },
    };
    try {
      if (editingId) await update({ id: editingId, body: input }).unwrap();
      else await create(input).unwrap();
      resetForm();
    } catch {
      /* surfaced below */
    }
  };

  const columns: Column<LaunchTemplate>[] = [
    { header: 'Template', render: (t) => t.template_name },
    {
      header: 'Metadata',
      render: (t) => {
        const m = metadataTemplates.find((x) => x.id === t.metadata_template_id);
        return m ? `${m.name} / ${m.symbol}` : <span className="muted">— unlinked —</span>;
      },
    },
    { header: 'Variant', render: (t) => <span className="mono text-xs">{t.variant}</span> },
    {
      header: 'Launchpad',
      render: (t) => launchpads.find((lp) => lp.id === t.launchpad_id)?.display_name ?? t.launchpad_id,
    },
    { header: 'Quote', render: (t) => quoteAssets.find((qa) => qa.id === t.quote_asset_id)?.symbol ?? t.quote_asset_id },
    { header: 'Legs', align: 'right', render: (t) => t.params.leg_structures?.length ?? t.params.bundle_leg_count ?? 0 },
    { header: 'Updated', align: 'right', render: (t) => <AgeCell iso={t.updated_at} /> },
    {
      header: '',
      align: 'right',
      render: (t) => (
        <div className="flex justify-end gap-2">
          <Button size="sm" onClick={() => onEdit(t)}>Edit</Button>
          <Button
            size="sm"
            variant="danger"
            loading={removeState.isLoading && removeState.originalArgs === t.id}
            onClick={() => onDelete(t)}
          >
            Delete
          </Button>
        </div>
      ),
    },
  ];

  const error =
    apiErrorMessage(createState.error) ??
    apiErrorMessage(updateState.error) ??
    apiErrorMessage(removeState.error);
  const saving = createState.isLoading || updateState.isLoading;

  return (
    <div className="space-y-4">
      <h1 className="text-lg font-semibold">Launch Templates</h1>

      <Card title={editingId ? 'Edit launch template' : 'Author launch template'}>
        {editingId && (
          <Banner tone="warn" className="mb-3" actions={<Button size="sm" onClick={resetForm}>Cancel</Button>}>
            Editing <strong>{templateName}</strong> — does not retroactively change past launches.
          </Banner>
        )}

        <div className="grid grid-cols-1 md:grid-cols-4 gap-3">
          <Field label="Template name" htmlFor="lt-tn">
            <Input id="lt-tn" value={templateName} onChange={(e) => setTemplateName(e.target.value)} placeholder="standard-v2-bundle" />
          </Field>
          <Field label="Launchpad" htmlFor="lt-lp">
            <Select id="lt-lp" value={launchpadId} onChange={(e) => setLaunchpadId(e.target.value)}>
              {launchpads.length === 0 && <option value="">No launchpads</option>}
              {launchpads.map((lp) => (
                <option key={lp.id} value={lp.id}>{lp.display_name}</option>
              ))}
            </Select>
          </Field>
          <Field label="Quote asset" htmlFor="lt-qa">
            <Select id="lt-qa" value={quoteAssetId} onChange={(e) => setQuoteAssetId(e.target.value)}>
              {quoteAssets.length === 0 && <option value="">No quote assets</option>}
              {quoteAssets.map((qa) => (
                <option key={qa.id} value={qa.id}>{qa.symbol}</option>
              ))}
            </Select>
          </Field>
          <Field label="Variant" htmlFor="lt-v">
            <Select id="lt-v" value={variant} onChange={(e) => setVariant(e.target.value)}>
              {VARIANTS.map((v) => (
                <option key={v} value={v}>{v}</option>
              ))}
            </Select>
          </Field>
          <Field label="Metadata template (name/symbol/URI)" htmlFor="lt-mt" className="md:col-span-4">
            <Select id="lt-mt" value={metadataTemplateId} onChange={(e) => setMetadataTemplateId(e.target.value)}>
              <option value="">— select a metadata template —</option>
              {metadataTemplates.map((m) => (
                <option key={m.id} value={m.id}>{m.template_name} ({m.name} / {m.symbol})</option>
              ))}
            </Select>
          </Field>
        </div>

        <div className="grid grid-cols-2 md:grid-cols-4 gap-3 mt-3">
          <Field label={`Dev buy (${quoteSymbol})`} htmlFor="lt-db">
            <Input id="lt-db" value={devBuyQuote} onChange={(e) => setDevBuyQuote(e.target.value)} placeholder="0.05" />
          </Field>
          <Field label="Slippage (bps)" htmlFor="lt-sl">
            <Input id="lt-sl" type="number" value={slippageBps} onChange={(e) => setSlippageBps(e.target.value)} />
          </Field>
          <div className="flex items-end gap-4">
            <Checkbox id="lt-mh" label="Mayhem" checked={isMayhemMode} onChange={(e) => setIsMayhemMode(e.target.checked)} />
            <Checkbox id="lt-cb" label="Cashback" checked={cashbackEnabled} onChange={(e) => setCashbackEnabled(e.target.checked)} />
          </div>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-3 gap-3 mt-3">
          <Field label="Default bundle leg count" htmlFor="lt-lc">
            <Input id="lt-lc" type="number" min={0} value={bundleLegCount} onChange={(e) => setBundleLegCount(e.target.value)} />
          </Field>
          <Field label={`Quote per leg (${quoteSymbol})`} htmlFor="lt-qpl">
            <Input id="lt-qpl" value={bundleQuotePerLeg} onChange={(e) => setBundleQuotePerLeg(e.target.value)} placeholder="0.02" />
          </Field>
          <Field label={`Bundle tip (${quoteSymbol})`} htmlFor="lt-bt">
            <Input id="lt-bt" value={bundleTipQuote} onChange={(e) => setBundleTipQuote(e.target.value)} placeholder="0.001" />
          </Field>
        </div>

        <div className="mt-3">
          <IxLayoutEditor
            label="Create tx ix layout (orders CU/tip around the fused create Core)"
            value={createLayout}
            onChange={setCreateLayout}
            kind="create"
          />
        </div>

        <div className="mt-4">
          <div className="flex items-center justify-between">
            <h3 className="text-xs font-semibold uppercase tracking-wider muted">Leg structures</h3>
            <Button size="sm" onClick={addLegRow}>+ Add leg</Button>
          </div>
          <p className="mt-1 text-xs muted">
            Blank fields fall back to the composer defaults: slippage 300–800 bps, cu_limit
            120k–180k, cu_price 150k–400k, tip 10k–100k lamports. CU/tip stay persona-jittered
            even for a hand-picked variant + layout.
          </p>
          {uniformLayoutWarning && (
            <Banner tone="warn" className="mt-2">
              ≥3 legs share an identical ix layout — the fingerprint auditor flags this
              (<span className="mono">UniformLayout</span>). Vary the step order across legs, or the
              launch needs <span className="mono">allow_fingerprint</span> to send.
            </Banner>
          )}
          <div className="mt-2 space-y-2">
            {legRows.map((row, i) => (
              <div key={i} className="panel p-3 grid grid-cols-2 md:grid-cols-6 gap-2 items-end">
                <Field label="Variant">
                  <Select value={row.variant} onChange={(e) => updateLegRow(i, { variant: e.target.value as BuyVariant })}>
                    {BUY_VARIANTS.map((v) => (
                      <option key={v} value={v}>{v}</option>
                    ))}
                  </Select>
                </Field>
                <LegRange label="Slippage bps" min={row.slippage_bps_min} max={row.slippage_bps_max}
                  onMin={(v) => updateLegRow(i, { slippage_bps_min: v })} onMax={(v) => updateLegRow(i, { slippage_bps_max: v })} />
                <LegRange label="CU limit" min={row.cu_limit_min} max={row.cu_limit_max}
                  onMin={(v) => updateLegRow(i, { cu_limit_min: v })} onMax={(v) => updateLegRow(i, { cu_limit_max: v })} />
                <LegRange label="CU price" min={row.cu_price_min} max={row.cu_price_max}
                  onMin={(v) => updateLegRow(i, { cu_price_min: v })} onMax={(v) => updateLegRow(i, { cu_price_max: v })} />
                <LegRange label={`Tip ${quoteSymbol}`} min={row.tip_quote_min} max={row.tip_quote_max}
                  onMin={(v) => updateLegRow(i, { tip_quote_min: v })} onMax={(v) => updateLegRow(i, { tip_quote_max: v })} />
                <Button variant="danger" size="sm" onClick={() => removeLegRow(i)}>Remove</Button>
                <div className="col-span-2 md:col-span-6">
                  <IxLayoutEditor label="Ix layout" value={row.layout}
                    onChange={(layout) => updateLegRow(i, { layout })} kind="buy" />
                </div>
              </div>
            ))}
          </div>
        </div>

        <div className="mt-4 flex items-center gap-3">
          <Button variant="primary" loading={saving} disabled={!canSubmit} onClick={onSubmit}>
            {editingId ? 'Save changes' : 'Create template'}
          </Button>
          {error && <Banner tone="bad">{error}</Banner>}
        </div>
      </Card>

      <Card title={`Templates (${templates.length})`}>
        <DataTable
          columns={columns}
          rows={templates}
          rowKey={(t) => t.id}
          loading={isFetching}
          empty="No launch templates yet — author one above."
        />
      </Card>
    </div>
  );
}

function LegRange({
  label,
  min,
  max,
  onMin,
  onMax,
}: {
  label: string;
  min: string;
  max: string;
  onMin: (v: string) => void;
  onMax: (v: string) => void;
}) {
  return (
    <Field label={label}>
      <div className="flex gap-1">
        <Input value={min} onChange={(e) => onMin(e.target.value)} placeholder="min" />
        <Input value={max} onChange={(e) => onMax(e.target.value)} placeholder="max" />
      </div>
    </Field>
  );
}
