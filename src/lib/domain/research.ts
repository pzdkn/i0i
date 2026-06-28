// Domain types for deep-research agentic search (RFC 0037).
// Shapes mirror the Rust serde (camelCase) in src-tauri/src/domain/research.rs.

export type Depth = "quick" | "standard" | "thorough";

export type SearchRunStatus =
  | "queued"
  | "planning"
  | "searching"
  | "assessing"
  | "ranking"
  | "ready"
  | "failed"
  | "cancelled";

export interface SearchStrategy {
  depth: Depth;
  maxIterations: number;
  maxProviderQueries: number;
  maxLlmCalls: number;
}

export interface SearchConstraints {
  yearFrom?: number;
  yearTo?: number;
  providers: string[];
  openAccess: boolean;
  targetCount: number;
  venues: string[];
  authors: string[];
  fieldsOfStudy: string[];
  seedPaperIds: string[];
}

export interface SearchDraft {
  title: string;
  goal: string;
  constraints: SearchConstraints;
  strategy: SearchStrategy;
}

export interface Search {
  id: string;
  title: string;
  goal: string;
  constraints: SearchConstraints;
  strategy: SearchStrategy;
  status: string;
  stopReason?: string;
  summary?: string;
  createdAt: string;
  updatedAt: string;
}

// The candidate payload as normalized by the discovery providers.
export interface ResearchPaper {
  id: string;
  title: string;
  authors: string[];
  abstract?: string;
  year?: number;
  venue?: string;
  citationCount?: number;
  doi?: string;
  externalUrl?: string;
  pdfUrl?: string;
}

export interface SearchCandidate {
  id: string;
  searchId: string;
  firstSeenRunId: string;
  rank: number;
  score?: number;
  rationale?: string;
  candidate: ResearchPaper;
  alreadyInLibrary: boolean;
  saved: boolean;
  seen: boolean;
  firstSeenAt: string;
}

export interface SearchUpdated {
  searchId: string;
  runId: string;
  status: string;
  message: string;
  iteration: number;
  found: number;
  unique: number;
  new: number;
}

// Depth presets — mirror Depth::budget() in research.rs.
export function depthStrategy(depth: Depth): SearchStrategy {
  switch (depth) {
    case "quick":
      return { depth, maxIterations: 1, maxProviderQueries: 3, maxLlmCalls: 4 };
    case "thorough":
      return { depth, maxIterations: 4, maxProviderQueries: 16, maxLlmCalls: 24 };
    default:
      return { depth: "standard", maxIterations: 3, maxProviderQueries: 8, maxLlmCalls: 12 };
  }
}

export function isTerminalStatus(status: string): boolean {
  return status === "ready" || status === "failed" || status === "cancelled";
}
