import type { Paper } from "$lib/domain/paper";
import type { ReaderDocument, ReaderMark, ReaderParagraph } from "$lib/domain/reader";

const sharedParagraphs: ReaderParagraph[] = [
  {
    id: "h1",
    kind: "heading",
    text: "Abstract",
  },
  {
    id: "p1",
    kind: "paragraph",
    text: "This mocked reader body stands in for extracted paper text. It lets us shape the reading workspace before committing to PDF rendering, text extraction, or annotation persistence.",
  },
  {
    id: "p2",
    kind: "paragraph",
    text: "The important product behavior is not the exact paper content yet. The important behavior is that a vault item opens into a stable reading surface with paragraph anchors, margin marks, metadata, and future study affordances.",
    highlight: "soft",
  },
  {
    id: "h2",
    kind: "heading",
    text: "1 Introduction",
  },
  {
    id: "p3",
    kind: "paragraph",
    text: "Researchers rarely read papers in isolation. They compare methods, trace lineages, collect uncertainties, and transform fragments into durable knowledge. i0i treats reading as a workspace inside a broader research vault.",
  },
  {
    id: "p4",
    kind: "paragraph",
    text: "A paragraph-level model is useful even in a mock because it gives notes, questions, highlights, and future citations something stable to attach to without coupling the UI to a specific PDF renderer.",
    highlight: "strong",
  },
  {
    id: "p5",
    kind: "paragraph",
    text: "Later, these paragraphs can be produced by a text extraction service. For now, frontend mock data is enough to validate the layout, interaction model, and visual hierarchy.",
    highlight: "soft",
  },
  {
    id: "h3",
    kind: "heading",
    text: "2 Reading Model",
  },
  {
    id: "p6",
    kind: "paragraph",
    text: "The reader should support clean text, original PDF, and a split mode. In this skeleton only text mode is active, while PDF and split remain visible as future affordances.",
  },
  {
    id: "p7",
    kind: "paragraph",
    text: "The surrounding panes are part of the reading experience: the left explorer anchors the paper in the vault, the margin records local marks, and the inspector exposes lineage, metadata, and question workflows.",
  },
];

const sharedMarks: ReaderMark[] = [
  {
    id: "m1",
    paragraphId: "p4",
    kind: "note",
    body: "Model marks by paragraph ID for now. Avoid page coordinates until PDF rendering is real.",
    createdLabel: "me / 2d",
  },
  {
    id: "m2",
    paragraphId: "p6",
    kind: "question",
    body: "When should split mode become real: before or after annotation persistence?",
    createdLabel: "me / 2d",
  },
  {
    id: "m3",
    paragraphId: "p7",
    kind: "highlight",
    body: "Reader is a workspace inside the vault, not a separate app.",
    createdLabel: "me / today",
  },
];

export function createReaderDocument(paper: Paper): ReaderDocument {
  return {
    paperId: paper.id,
    title: paper.title,
    authors: paper.authors,
    venue: paper.venue,
    year: paper.year,
    identifier: paper.id === "vaswani2017" ? "arXiv:1706.03762" : `mock:${paper.id}`,
    citationKey: paper.id,
    tags: paper.tags,
    paragraphs: sharedParagraphs,
    marks: sharedMarks,
  };
}
