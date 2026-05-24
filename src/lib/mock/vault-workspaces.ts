import type { VaultWorkspace } from "$lib/domain/library";
import { papers } from "$lib/mock/papers";

const byId = Object.fromEntries(papers.map((paper) => [paper.id, paper]));

function pick(ids: string[]) {
  return ids.map((id) => byId[id]).filter(Boolean);
}

export const vaultWorkspaces: VaultWorkspace[] = [
  {
    id: "attention",
    title: "attention",
    path: "/transformers/attention",
    summary: "22 papers / 3 unread / last add 2d",
    tabs: [
      { label: "Papers", count: 22 },
      { label: "Notes", count: 14 },
      { label: "Annotations", count: 87 },
      { label: "Graph" },
      { label: "Q&A" },
    ],
    chips: ["foundational x8", "transformer x22", "attention x22", "NeurIPS x6", "ICLR x4", "2017-2024"],
    papers,
  },
  {
    id: "self-supervised",
    title: "self-supervised",
    path: "/self-supervised",
    summary: "41 papers / 6 unread / last add today",
    tabs: [
      { label: "Papers", count: 41 },
      { label: "Notes", count: 18 },
      { label: "Annotations", count: 63 },
      { label: "Graph" },
      { label: "Q&A" },
    ],
    chips: ["ssl x41", "dino x7", "contrastive x12", "masked x11", "CVPR x9", "2020-2024"],
    papers: pick(["caron2021", "he2022", "chen2020", "grill2020", "oquab2023", "assran2023", "dosovitskiy2020"]),
  },
  {
    id: "vision-transformers",
    title: "vision-transformers",
    path: "/vision-transformers",
    summary: "33 papers / 4 unread / last add 4d",
    tabs: [
      { label: "Papers", count: 33 },
      { label: "Notes", count: 11 },
      { label: "Annotations", count: 42 },
      { label: "Graph" },
      { label: "Q&A" },
    ],
    chips: ["vit x33", "ssl x12", "frontier x9", "ICLR x6", "CVPR x8", "2021-2024"],
    papers: pick(["dosovitskiy2020", "caron2021", "he2022", "oquab2023", "assran2023"]),
  },
  {
    id: "interpretability",
    title: "interpretability",
    path: "/interpretability",
    summary: "28 papers / 5 unread / last add 1w",
    tabs: [
      { label: "Papers", count: 28 },
      { label: "Notes", count: 9 },
      { label: "Annotations", count: 31 },
      { label: "Graph" },
      { label: "Q&A" },
    ],
    chips: ["interp x28", "circuits x12", "mechanistic x8", "frontier x6", "2020-2024"],
    papers: pick(["vaswani2017", "devlin2018", "radford2019", "kaplan2020", "tay2022"]),
  },
  {
    id: "scaling",
    title: "scaling-laws",
    path: "/transformers/scaling-laws",
    summary: "14 papers / 2 unread / last add 6d",
    tabs: [
      { label: "Papers", count: 14 },
      { label: "Notes", count: 7 },
      { label: "Annotations", count: 22 },
      { label: "Graph" },
      { label: "Q&A" },
    ],
    chips: ["scaling x14", "foundational x4", "language x9", "arXiv x6", "2020-2024"],
    papers: pick(["kaplan2020", "radford2019", "devlin2018", "tay2022", "vaswani2017"]),
  },
];
