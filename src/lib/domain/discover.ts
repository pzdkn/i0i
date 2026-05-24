export type DiscoverCandidate = {
  id: string;
  title: string;
  authors: string[];
  venue: string;
  year: number;
  citations: number;
  score: number;
  why: string;
  tags: string[];
  owned?: boolean;
  isNew?: boolean;
};

export type DiscoverWorkspace = {
  id: string;
  title: string;
  seeds: string[];
  candidates: DiscoverCandidate[];
};
