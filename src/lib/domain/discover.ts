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

export type DiscoveryProviderChoice = "open_alex" | "arxiv" | "semantic_scholar";

const PROVIDER_DISPLAY_NAMES: Record<DiscoveryProviderChoice, string> = {
  open_alex: "OpenAlex",
  arxiv: "arXiv",
  semantic_scholar: "Semantic Scholar",
};

export function providerDisplayName(provider: DiscoveryProviderChoice): string {
  return PROVIDER_DISPLAY_NAMES[provider] ?? "Unknown";
}

export type DiscoverRunStatus = "idle" | "running" | "completed" | "failed";

export type DiscoverRunMode = "shallow" | "deep";

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
