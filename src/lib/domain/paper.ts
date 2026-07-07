export type PaperStatus = "READ" | "READING" | "UNREAD";

export type Paper = {
  id: string;
  title: string;
  authors: string[];
  venue: string;
  year: number;
  citations: number;
  tags: string[];
  highlightCount: number;
  annotationCount: number;
  status: PaperStatus;
  abstract?: string;
  activeSourceId?: string;
  activeExtractionId?: string;
};
