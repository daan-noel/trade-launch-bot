import { baseApi } from './baseApi';
import type {
  BootstrapPayload,
  Bundle,
  FundReport,
  IngestStatus,
  Launch,
  LaunchesPage,
  Launchpad,
  LaunchOverrides,
  LaunchResult,
  LaunchStatus,
  LaunchTemplate,
  ManagedWalletPool,
  MetadataTemplate,
  NewLaunchTemplateInput,
  NewMetadataTemplate,
  UpdateMetadataTemplate,
  QuoteAsset,
  TokenOverview,
  TokenPosition,
  TradePriced,
} from '@shared/types';

const q = (params: Record<string, string | number | undefined>) => {
  const s = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) if (v !== undefined) s.set(k, String(v));
  const str = s.toString();
  return str ? `?${str}` : '';
};

export const api = baseApi.injectEndpoints({
  endpoints: (build) => ({
    // ---- Composite / dimensions ----
    bootstrap: build.query<BootstrapPayload, void>({
      query: () => '/api/bootstrap',
      providesTags: ['Bootstrap', 'Templates', 'MetadataTemplates', 'Wallets'],
    }),
    launchpads: build.query<Launchpad[], void>({
      query: () => '/api/launchpads',
      providesTags: ['Dimensions'],
    }),
    quoteAssets: build.query<QuoteAsset[], void>({
      query: () => '/api/quote_assets',
      providesTags: ['Dimensions'],
    }),

    // ---- Ingest toggle ----
    ingestStatus: build.query<IngestStatus, void>({
      query: () => '/api/ingest',
      providesTags: ['Ingest'],
    }),
    setIngest: build.mutation<IngestStatus, boolean>({
      query: (live) => ({ url: '/api/ingest', method: 'PUT', body: { live } }),
      invalidatesTags: ['Ingest'],
    }),

    // ---- Launch templates ----
    templates: build.query<LaunchTemplate[], void>({
      query: () => '/api/launch_templates',
      providesTags: ['Templates'],
    }),
    createTemplate: build.mutation<LaunchTemplate, NewLaunchTemplateInput>({
      query: (body) => ({ url: '/api/launch_templates', method: 'POST', body }),
      invalidatesTags: ['Templates', 'Bootstrap'],
    }),
    updateTemplate: build.mutation<
      LaunchTemplate,
      { id: string; body: NewLaunchTemplateInput }
    >({
      query: ({ id, body }) => ({ url: `/api/launch_templates/${id}`, method: 'PUT', body }),
      invalidatesTags: ['Templates', 'Bootstrap'],
    }),
    deleteTemplate: build.mutation<void, string>({
      query: (id) => ({ url: `/api/launch_templates/${id}`, method: 'DELETE' }),
      invalidatesTags: ['Templates', 'Bootstrap'],
    }),

    // ---- Metadata templates ----
    metadataTemplates: build.query<MetadataTemplate[], void>({
      query: () => '/api/metadata_templates',
      providesTags: ['MetadataTemplates'],
    }),
    createMetadataTemplate: build.mutation<MetadataTemplate, NewMetadataTemplate>({
      query: (body) => ({ url: '/api/metadata_templates', method: 'POST', body }),
      invalidatesTags: ['MetadataTemplates', 'Bootstrap'],
    }),
    updateMetadataTemplate: build.mutation<
      MetadataTemplate,
      { id: string; body: UpdateMetadataTemplate }
    >({
      query: ({ id, body }) => ({ url: `/api/metadata_templates/${id}`, method: 'PUT', body }),
      invalidatesTags: ['MetadataTemplates', 'Bootstrap'],
    }),
    deleteMetadataTemplate: build.mutation<void, string>({
      query: (id) => ({ url: `/api/metadata_templates/${id}`, method: 'DELETE' }),
      // Launch templates lose their link (FK SET NULL) — refresh Templates too.
      invalidatesTags: ['MetadataTemplates', 'Templates', 'Bootstrap'],
    }),

    // ---- Wallet pool ----
    walletPool: build.query<ManagedWalletPool[], string | undefined>({
      query: (role) => `/api/wallet_pool${role ? q({ role }) : ''}`,
      providesTags: ['Wallets'],
    }),
    generateWallets: build.mutation<
      ManagedWalletPool[],
      { role: string; count: number; label_prefix?: string }
    >({
      query: (body) => ({ url: '/api/wallet_pool/generate', method: 'POST', body }),
      invalidatesTags: ['Wallets', 'Bootstrap'],
    }),
    fundPool: build.mutation<FundReport, { role?: string; count?: number }>({
      query: (body) => ({ url: '/api/wallet_pool/fund', method: 'POST', body }),
      invalidatesTags: ['Wallets'],
    }),
    fundForLaunch: build.mutation<
      FundReport,
      { template_id: string; dev_wallet_id: string; bundler_count?: number }
    >({
      query: (body) => ({ url: '/api/wallet_pool/fund_for_launch', method: 'POST', body }),
      invalidatesTags: ['Wallets', 'Bootstrap'],
    }),
    // DANGER: returns raw base58 private key. Secret goes in the X-Export-Secret
    // header (never the URL/body cache). No tag invalidation — pure read, and we
    // do NOT want the key retained anywhere in the RTK cache.
    exportWalletKey: build.mutation<
      { address: string; private_key_base58: string },
      { id: string; secret: string }
    >({
      query: ({ id, secret }) => ({
        url: `/api/wallet_pool/${id}/export`,
        method: 'POST',
        headers: { 'X-Export-Secret': secret },
      }),
    }),

    // ---- Launches ----
    launches: build.query<LaunchesPage, { limit?: number; offset?: number } | void>({
      query: (arg) => `/api/launches${q({ limit: arg?.limit, offset: arg?.offset })}`,
      providesTags: ['Launches'],
    }),
    executeLaunch: build.mutation<
      LaunchResult,
      { template_id: string; dev_wallet_id: string; overrides?: LaunchOverrides }
    >({
      query: ({ template_id, dev_wallet_id, overrides }) => ({
        url: '/api/launches/execute',
        method: 'POST',
        body: { template_id, dev_wallet_id, ...overrides },
      }),
      invalidatesTags: ['Launches', 'Wallets'],
    }),
    launch: build.query<Launch, string>({
      query: (id) => `/api/launches/${id}`,
    }),
    launchStatus: build.query<LaunchStatus, string>({
      query: (id) => `/api/launches/${id}/status`,
    }),
    executeBundle: build.mutation<Bundle, string>({
      query: (id) => ({ url: `/api/bundles/${id}/execute`, method: 'POST' }),
      invalidatesTags: ['Launches'],
    }),

    // ---- Token detail ----
    tokenOverview: build.query<TokenOverview, string>({
      query: (mint) => `/api/tokens/${mint}/overview`,
    }),
    tokenTrades: build.query<TradePriced[], { mint: string; limit?: number }>({
      query: ({ mint, limit }) => `/api/tokens/${mint}/trades${q({ limit })}`,
    }),
    tokenPositions: build.query<TokenPosition[], string>({
      query: (mint) => `/api/tokens/${mint}/positions`,
      providesTags: ['Positions'],
    }),
  }),
  overrideExisting: false,
});

export const {
  useBootstrapQuery,
  useLaunchpadsQuery,
  useQuoteAssetsQuery,
  useIngestStatusQuery,
  useSetIngestMutation,
  useTemplatesQuery,
  useCreateTemplateMutation,
  useUpdateTemplateMutation,
  useDeleteTemplateMutation,
  useMetadataTemplatesQuery,
  useCreateMetadataTemplateMutation,
  useUpdateMetadataTemplateMutation,
  useDeleteMetadataTemplateMutation,
  useWalletPoolQuery,
  useGenerateWalletsMutation,
  useFundPoolMutation,
  useFundForLaunchMutation,
  useExportWalletKeyMutation,
  useLaunchesQuery,
  useExecuteLaunchMutation,
  useLaunchQuery,
  useLaunchStatusQuery,
  useExecuteBundleMutation,
  useTokenOverviewQuery,
  useTokenTradesQuery,
  useTokenPositionsQuery,
} = api;
