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
};

export type DiscoverSort = "relevance" | "newest" | "most_cited";

export type DiscoveryProviderChoice = "open_alex" | "arxiv";

export type DiscoverRunStatus = "idle" | "running" | "completed" | "failed";

export type DiscoverWorkspace = {
  id: string;
  title: string;
  seeds: string[];
  query: string;
  yearFrom: string;
  yearTo: string;
  resultLimit: 10 | 25 | 50;
  sortBy: DiscoverSort;
  provider: DiscoveryProviderChoice;
  status: DiscoverRunStatus;
  error: string;
  lastRun?: {
    provider: string;
    query: string;
    filters: string[];
    resultCount: number;
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
