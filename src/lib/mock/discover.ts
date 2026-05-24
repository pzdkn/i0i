import type { DiscoverWorkspace } from "$lib/domain/discover";
import type { Paper } from "$lib/domain/paper";

export const discoverWorkspaces: DiscoverWorkspace[] = [
  {
    id: "ssl-dino",
    title: "Discover: ssl + dino",
    seeds: ["@caron2021", "/self-supervised", "#frontier", "year:>2022"],
    candidates: [
      {
        id: "darcet2024",
        title: "Vision Transformers Need Registers",
        authors: ["T. Darcet", "M. Oquab", "J. Mairal", "P. Bojanowski"],
        venue: "ICLR",
        year: 2024,
        citations: 412,
        score: 0.94,
        why: "Cited by 3 papers in your DINO folder / cites Caron 2021",
        tags: ["vit", "ssl", "frontier"],
        isNew: true,
      },
      {
        id: "assran2023-ijepa",
        title: "I-JEPA: Image-based Joint-Embedding Predictive Architecture",
        authors: ["M. Assran", "Q. Duval", "I. Misra", "P. Vincent"],
        venue: "CVPR",
        year: 2023,
        citations: 612,
        score: 0.91,
        why: "Same authors as 4 papers you have / LeCun cluster",
        tags: ["ssl", "jepa", "frontier"],
      },
      {
        id: "liu2022-convnext",
        title: "A ConvNet for the 2020s",
        authors: ["Z. Liu", "H. Mao", "C. Wu", "C. Feichtenhofer"],
        venue: "CVPR",
        year: 2022,
        citations: 4283,
        score: 0.87,
        why: "Often co-cited with ViT and DINO papers",
        tags: ["vit", "convnext"],
      },
      {
        id: "dehghani2023-vit22b",
        title: "Scaling Vision Transformers to 22 Billion Parameters",
        authors: ["M. Dehghani", "B. Mustafa", "J. Djolonga", "J. Heek"],
        venue: "ICML",
        year: 2023,
        citations: 423,
        score: 0.86,
        why: "Extends a paper you starred / scaling-laws folder",
        tags: ["vit", "scaling", "frontier"],
      },
      {
        id: "assran2022-msn",
        title: "Masked Siamese Networks for Label-Efficient Learning",
        authors: ["M. Assran", "M. Caron", "I. Misra", "P. Bojanowski"],
        venue: "ECCV",
        year: 2022,
        citations: 514,
        score: 0.83,
        why: "Bridges your contrastive and masked-modeling folders",
        tags: ["ssl", "contrastive", "masked"],
      },
      {
        id: "caron2021",
        title: "Emerging Properties in Self-Supervised Vision Transformers",
        authors: ["M. Caron", "H. Touvron", "I. Misra", "H. Jegou"],
        venue: "ICCV",
        year: 2021,
        citations: 8842,
        score: 0.99,
        why: "In your vault / self-supervised / dino-family",
        tags: ["ssl", "dino", "vit"],
        owned: true,
      },
      {
        id: "bao2022-beit",
        title: "BEiT: BERT Pre-Training of Image Transformers",
        authors: ["H. Bao", "L. Dong", "S. Piao", "F. Wei"],
        venue: "ICLR",
        year: 2022,
        citations: 2104,
        score: 0.81,
        why: "Cited by Caron 2021 background section",
        tags: ["ssl", "masked", "vit"],
      },
    ],
  },
];

export function getDiscoverWorkspace(discoverId: string) {
  return discoverWorkspaces.find((workspace) => workspace.id === discoverId) ?? discoverWorkspaces[0];
}

export function discoverCandidateToPaper(candidateId: string): Paper {
  const workspace = discoverWorkspaces[0];
  const candidate = workspace.candidates.find((item) => item.id === candidateId) ?? workspace.candidates[0];

  return {
    id: candidate.id,
    title: candidate.title,
    authors: candidate.authors,
    venue: candidate.venue,
    year: candidate.year,
    citations: candidate.citations,
    tags: candidate.tags,
    noteCount: 0,
    annotationCount: 0,
    status: candidate.owned ? "READ" : "UNREAD",
    abstract: candidate.why,
  };
}
