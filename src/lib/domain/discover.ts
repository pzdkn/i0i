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
  /** Probed PDF availability (RFC 0051); unset until a probe has run. */
  pdfAvailability?: "verified" | "browser_required" | "unavailable";
};

export type DiscoverSort = "relevance" | "newest" | "most_cited";

export type DiscoveryProviderChoice = "open_alex" | "arxiv" | "europe_pmc" | "core";

// Supported providers. OpenAlex + arXiv (RFC 0044); Europe PMC + CORE added
// opt-in (RFC 0053). Semantic Scholar was removed.
export const SUPPORTED_PROVIDERS: DiscoveryProviderChoice[] = [
  "open_alex",
  "arxiv",
  "europe_pmc",
  "core",
];

// Default provider set. The RFC 0053 providers are opt-in, so a stale/empty
// provider list falls back to just OpenAlex + arXiv, not the full set.
export const DEFAULT_PROVIDERS: DiscoveryProviderChoice[] = ["open_alex", "arxiv"];

const PROVIDER_DISPLAY_NAMES: Record<string, string> = {
  web: "Web",
  browser: "Web",
  open_alex: "OpenAlex",
  openalex: "OpenAlex",
  arxiv: "arXiv",
  europe_pmc: "Europe PMC",
  core: "CORE",
};

export function providerDisplayName(provider: DiscoveryProviderChoice | string): string {
  return PROVIDER_DISPLAY_NAMES[provider] ?? "Unknown";
}

/// Drop stale/removed provider values (e.g. `semantic_scholar` from RFC 0043
/// state); fall back to the default set if nothing valid remains.
export function sanitizeProviders(providers: readonly string[]): DiscoveryProviderChoice[] {
  const valid = providers.filter((provider): provider is DiscoveryProviderChoice =>
    (SUPPORTED_PROVIDERS as string[]).includes(provider),
  );
  return valid.length > 0 ? valid : [...DEFAULT_PROVIDERS];
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
  /** Hide results with no obtainable PDF/HTML view (RFC 0053). */
  onlyViewable: boolean;
  /** Run cheap-LLM query expansion after literal quick-search results (RFC 0054). */
  expandSearch: boolean;
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
  onlyViewable?: boolean;
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

export type DiscoveryProgress = {
  query: string;
  stage: "searching" | "provisional" | "resolving" | "resolved" | "ranking";
  message: string;
  candidates: DiscoverySearchResponse["candidates"];
};
