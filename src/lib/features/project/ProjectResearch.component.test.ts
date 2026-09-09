/** Local inspector interactions against actual Svelte markup and mocked Tauri IPC. */
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import { SvelteMap } from "svelte/reactivity";
import type { ResearchEntryDetail, ResearchEntrySummary, ResearchStateSnapshot } from "$lib/domain/research-state";
import ProjectResearch from "./ProjectResearch.svelte";

const ipc = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(async () => () => {}) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: ipc.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: ipc.listen }));

let app: ReturnType<typeof mount>;
let target: HTMLDivElement;
let pending: Promise<ResearchEntryDetail> | undefined;
let project: SvelteMap<string, string>;

/** Build a readable State entry with an intentionally opaque storage key. */
function entry(id: string, text: string): ResearchEntrySummary {
  return { id, text, projectId: "project", kind: "finding", epistemicStatus: "source_supported",
    lifecycle: "active", firstRevision: 1, lastRevision: 2, evidenceCount: 0,
    relationCount: 0, contextCount: 0, createdAt: "now", updatedAt: "now" };
}
const entries = [entry("research_entry_a", "A bounded gap"),
  entry("research_entry_b", "A source-supported premise " + "with a long statement ".repeat(30)),
  entry("research_entry_c", "A second premise")];
entries[0].kind = "gap";
entries[1].lifecycle = "superseded";

/** Mirror the selected revision, including the older statement in historical views. */
function state(revision = 2, projectId = "project"): ResearchStateSnapshot {
  return { projectId, revision, currentRevision: 2,
    revisions: [1, 2].map((revision) => ({ projectId: "project", revision, reason: "Snapshot", createdAt: "now" })),
    entries: entries.map((item) => ({ ...item, projectId, text: revision === 1 ? `Historical ${item.text}` : item.text })) };
}

/** Return relation chains plus an unavailable target without exposing its id in UI. */
function detail(id: string, revision = 2): ResearchEntryDetail {
  const item = state(revision).entries.find((entry) => entry.id === id)!;
  const targets = id === entries[0].id ? [entries[1].id, "research_entry_missing"] : id === entries[1].id ? [entries[2].id] : [];
  return { entry: item, evidence: [], context: [], history: [],
    relations: targets.map((targetEntryId, index) => ({ id: `${id}:${index}`, entryId: id, targetEntryId, stateRevision: revision, kind: "derived_from" })) };
}

/** Find a required element or fail at its selector instead of a later null access. */
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const result = target.querySelector<T>(selector);
  expect(result, selector).not.toBeNull();
  return result!;
}

/** Wait for the real inspector's asynchronous bridge and Svelte updates. */
async function openRoot(): Promise<void> {
  await vi.waitFor(() => expect(target.querySelectorAll(".entry-content")).toHaveLength(3));
  element<HTMLButtonElement>(".entry-content").click();
  await vi.waitFor(() => expect(element(".detail-text").textContent).toBe(entries[0].text));
}

beforeEach(() => {
  pending = undefined;
  ipc.invoke.mockReset();
  ipc.invoke.mockImplementation(async (command: string, args: { entryId?: string; revision?: number; projectId?: string }) => {
    if (command === "get_research_state") return state(args.revision, args.projectId);
    if (command === "get_research_entry") return pending ?? detail(args.entryId!, args.revision);
    if (command === "get_research_harness") return { runs: [], events: [], harness: {
      status: "idle", configuration: { researchInstructions: "Study evidence", paperBudget: 10,
        goal: "", scope: "", exclusions: "", preferredConcepts: [], excludedConcepts: [],
        schedule: { enabled: false, cadence: "daily", localTime: "09:00", timezone: "UTC" } } } };
    if (command.startsWith("list_")) return [];
    throw new Error(`Unexpected test IPC: ${command}`);
  });
  target = document.createElement("div");
  document.body.append(target);
  project = new SvelteMap([["id", "project"]]);
  app = mount(ProjectResearch, { target, props: { get projectId() { return project.get("id")!; }, projectTitle: "Fixture" } });
  flushSync();
});

afterEach(async () => { await unmount(app); target.remove(); vi.restoreAllMocks(); });

test("relations show full accessible statements, not ids, even when list filters hide targets", async () => {
  await openRoot();
  const input = element<HTMLInputElement>('input[aria-label="Search Research State"]');
  input.value = entries[0].text;
  input.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
  expect(target.querySelectorAll(".entry-content")).toHaveLength(1);
  expect(element(".relation-link").getAttribute("aria-label")).toContain(entries[1].text);
  expect(element(".relation-preview").textContent).toBe(entries[1].text);
  expect(element('[aria-label="Entry relationships"]').textContent).not.toContain("research_entry_");
  expect(element('[aria-label="Entry relationships"]').textContent).toContain("Entry unavailable");
});

test("following premises and Back preserve the trail and restore link and list focus", async () => {
  await openRoot();
  element<HTMLButtonElement>(".relation-link").click();
  await vi.waitFor(() => expect(element(".detail-text").textContent).toBe(entries[1].text));
  expect(document.activeElement).toBe(element(".detail-heading h2"));
  element<HTMLButtonElement>(".relation-link").click();
  await vi.waitFor(() => expect(element(".detail-text").textContent).toBe(entries[2].text));
  element<HTMLButtonElement>('button[aria-label="Back"]').click();
  await vi.waitFor(() => expect(document.activeElement).toBe(element(".relation-link")));
  expect(element(".detail-text").textContent).toBe(entries[1].text);
  element<HTMLButtonElement>('button[aria-label="Back"]').click();
  await vi.waitFor(() => expect(element(".detail-text").textContent).toBe(entries[0].text));
  element<HTMLButtonElement>('button[aria-label="Back"]').click();
  await vi.waitFor(() => expect(document.activeElement).toBe(element(".entry-content")));
});

test("failed navigation keeps the current detail and does not push history", async () => {
  await openRoot();
  ipc.invoke.mockRejectedValueOnce(new Error("Entry could not be loaded"));
  element<HTMLButtonElement>(".relation-link").click();
  await vi.waitFor(() => expect(target.textContent).toContain("Entry could not be loaded"));
  expect(element(".detail-text").textContent).toBe(entries[0].text);
  element<HTMLButtonElement>('button[aria-label="Back"]').click();
  await vi.waitFor(() => expect(target.querySelector(".detail-text")).toBeNull());
});

test("duplicate clicks issue one request and Back discards its late response", async () => {
  await openRoot();
  let resolve!: (value: ResearchEntryDetail) => void;
  pending = new Promise((done) => { resolve = done; });
  ipc.invoke.mockClear();
  const link = element<HTMLButtonElement>(".relation-link");
  link.click(); link.click();
  expect(ipc.invoke).toHaveBeenCalledTimes(1);
  expect(element(".detail-text").textContent).toBe(entries[0].text);
  element<HTMLButtonElement>('button[aria-label="Back"]').click();
  resolve(detail(entries[1].id));
  await new Promise((done) => setTimeout(done, 0));
  flushSync();
  expect(target.querySelector(".detail-text")).toBeNull();
});

test("historical links load the selected revision and stale responses cannot replace it", async () => {
  await openRoot();
  let resolve!: (value: ResearchEntryDetail) => void;
  pending = new Promise((done) => { resolve = done; });
  element<HTMLButtonElement>(".relation-link").click();
  const revision = element<HTMLSelectElement>('select[aria-label="Research State revision"]');
  revision.value = "1";
  revision.dispatchEvent(new Event("change", { bubbles: true }));
  await vi.waitFor(() => expect(element(".entry-content").textContent).toContain("Historical"));
  resolve(detail(entries[1].id)); pending = undefined;
  await new Promise((done) => setTimeout(done, 0));
  expect(target.querySelector(".detail-text")).toBeNull();
  element<HTMLButtonElement>(".entry-content").click();
  await vi.waitFor(() => expect(element(".relation-link").textContent).toContain("Historical"));
  element<HTMLButtonElement>(".relation-link").click();
  await vi.waitFor(() => expect(element(".detail-text").textContent).toBe(`Historical ${entries[1].text}`));
  expect(ipc.invoke).toHaveBeenCalledWith("get_research_entry", { entryId: entries[1].id, revision: 1 });
  expect([...target.querySelectorAll(".details button")].some((button) => button.textContent === "Revise")).toBe(false);
});

test("switching Project discards pending details from the previous Project", async () => {
  await openRoot();
  let resolve!: (value: ResearchEntryDetail) => void;
  pending = new Promise((done) => { resolve = done; });
  element<HTMLButtonElement>(".relation-link").click();
  project.set("id", "another-project");
  flushSync();
  await vi.waitFor(() => expect(ipc.invoke).toHaveBeenCalledWith("get_research_state", { projectId: "another-project", revision: undefined }));
  resolve(detail(entries[1].id));
  await new Promise((done) => setTimeout(done, 0));
  expect(target.querySelector(".detail-text")).toBeNull();
});

test("a lifecycle edit keeps the updated detail while discarding the old-revision trail", async () => {
  await openRoot();
  element<HTMLButtonElement>(".relation-link").click();
  await vi.waitFor(() => expect(element(".detail-text").textContent).toBe(entries[1].text));
  vi.spyOn(window, "prompt").mockReturnValue("Contradictory evidence");
  const updated = detail(entries[1].id);
  updated.entry = { ...updated.entry, lifecycle: "contested", lastRevision: 3 };
  ipc.invoke.mockResolvedValueOnce({ entry: updated, state: { ...state(3), currentRevision: 3 } });
  [...target.querySelectorAll<HTMLButtonElement>(".details button")].find((button) => button.textContent === "Contest")!.click();
  await vi.waitFor(() => expect(element(".detail-meta").textContent).toContain("contested"));
  expect(element(".detail-text").textContent).toBe(entries[1].text);
  element<HTMLButtonElement>('button[aria-label="Back"]').click();
  await vi.waitFor(() => expect(target.querySelector(".detail-text")).toBeNull());
});
