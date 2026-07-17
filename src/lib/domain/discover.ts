export type DiscoverCandidate = {
  id: string;
  sourceProvider?: string;
  sourceId?: string;
  title: string;
  authors: string[];
  venue: string;
  year: number;
  citations: number;
  score: number;
  why: string;
  tags: string[];
  abstract?: string;
  publicationDate?: string;
  doi?: string;
  openalexId?: string;
  arxivId?: string;
  externalUrl?: string;
  pdfUrl?: string;
  openAccess?: {
    isOpenAccess: boolean;
    status?: string;
  };
  match?: {
    score?: number;
    reasons: string[];
    matchedKeywords: string[];
    fromSeedPaperIds: string[];
  };
  owned?: boolean;
  alreadyInLibrary?: boolean;
  isNew?: boolean;
  reviewing?: boolean;
};

export type DiscoverSort = "relevance" | "newest" | "most_cited";

export type DiscoveryProviderChoice = "open_alex" | "arxiv";

// Supported providers (RFC 0044). Semantic Scholar was removed.
export const SUPPORTED_PROVIDERS: DiscoveryProviderChoice[] = ["open_alex", "arxiv"];

const PROVIDER_DISPLAY_NAMES: Record<string, string> = {
  open_alex: "OpenAlex",
  openalex: "OpenAlex",
  arxiv: "arXiv",
};

export function providerDisplayName(provider: DiscoveryProviderChoice | string): string {
  return PROVIDER_DISPLAY_NAMES[provider] ?? "Unknown";
}

/// Drop stale/removed provider values (e.g. `semantic_scholar` from RFC 0043
/// state); fall back to the full supported set if nothing valid remains.
export function sanitizeProviders(providers: readonly string[]): DiscoveryProviderChoice[] {
  const valid = providers.filter((provider): provider is DiscoveryProviderChoice =>
    (SUPPORTED_PROVIDERS as string[]).includes(provider),
  );
  return valid.length > 0 ? valid : [...SUPPORTED_PROVIDERS];
}

export type DiscoverRunStatus = "idle" | "running" | "completed" | "failed";

export type DiscoverRunMode = "shallow" | "deep" | "improve";

export type DiscoverDepth = "quick" | "standard" | "thorough";

export type DiscoverRunProgress = {
  message: string;
  iteration: number;
  found: number;
  unique: number;
  new: number;
};

export type DiscoverWorkspace = {
  id: string;
  title: string;
  seeds: string[];
  query: string;
  deep: boolean;
  deepDepth: DiscoverDepth;
  yearFrom: string;
  yearTo: string;
  resultLimit: 10 | 25 | 50;
  sortBy: DiscoverSort;
  provider: DiscoveryProviderChoice;
  providers: DiscoveryProviderChoice[];
  venue: string;
  openAccess: boolean;
  status: DiscoverRunStatus;
  error: string;
  activeRunMode?: DiscoverRunMode;
  researchSearchId?: string;
  researchRunId?: string;
  runTrace: string[];
  runProgress?: DiscoverRunProgress;
  lastRun?: {
    provider: string;
    query: string;
    filters: string[];
    resultCount: number;
    mode?: DiscoverRunMode;
  };
  selectedCandidateId?: string;
  candidates: DiscoverCandidate[];
};

export type DiscoverySearchRequest = {
  query: string;
  yearFrom?: number;
  yearTo?: number;
  resultLimit: number;
  sortBy: DiscoverSort;
  provider: DiscoveryProviderChoice;
  providers?: DiscoveryProviderChoice[];
  openAccess?: boolean;
  venues?: string[];
};

export type DiscoverySearchResponse = {
  provider: string;
  query: string;
  filters: string[];
  sortBy: DiscoverSort;
  resultLimit: number;
  resultCount: number;
  candidates: Array<{
    id: string;
    sourceProvider: string;
    sourceId: string;
    title: string;
    authors: string[];
    abstract?: string;
    year?: number;
    publicationDate?: string;
    venue?: string;
    citationCount?: number;
    doi?: string;
    openalexId?: string;
    arxivId?: string;
    externalUrl?: string;
    pdfUrl?: string;
    openAccess?: {
      isOpenAccess: boolean;
      status?: string;
    };
    matchSummary: {
      score?: number;
      reasons: string[];
      matchedKeywords: string[];
      fromSeedPaperIds: string[];
    };
    alreadyInLibrary: boolean;
  }>;
};
