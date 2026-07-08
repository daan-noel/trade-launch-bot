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
  QuoteAsset,
  TokenOverview,
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

    // ---- Metadata templates ----
    metadataTemplates: build.query<MetadataTemplate[], void>({
      query: () => '/api/metadata_templates',
      providesTags: ['MetadataTemplates'],
    }),
    createMetadataTemplate: build.mutation<MetadataTemplate, NewMetadataTemplate>({
      query: (body) => ({ url: '/api/metadata_templates', method: 'POST', body }),
      invalidatesTags: ['MetadataTemplates', 'Bootstrap'],
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
  useMetadataTemplatesQuery,
  useCreateMetadataTemplateMutation,
  useWalletPoolQuery,
  useGenerateWalletsMutation,
  useFundPoolMutation,
  useFundForLaunchMutation,
  useLaunchesQuery,
  useExecuteLaunchMutation,
  useLaunchQuery,
  useLaunchStatusQuery,
  useExecuteBundleMutation,
  useTokenOverviewQuery,
  useTokenTradesQuery,
} = api;
