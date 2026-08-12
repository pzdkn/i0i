import { invoke } from "@tauri-apps/api/core";
import type { Highlight, HighlightColor, Locator } from "$lib/domain/highlight";

export function createHighlight(args: {
  paperId: string;
  locator: Locator;
  excerpt: string;
  // Optional (RFC 0061): omit/null to create a note-/ask-only passage (no color mark).
  color?: HighlightColor | null;
  label?: string | null;
}): Promise<Highlight> {
  return invoke<Highlight>("create_highlight", {
    paperId: args.paperId, locator: args.locator, excerpt: args.excerpt,
    color: args.color ?? null, label: args.label ?? null,
  });
}

export function recolorHighlight(id: string, color: HighlightColor): Promise<void> {
  return invoke("recolor_highlight", { id, color });
}

export function setHighlightLabel(id: string, label: string | null): Promise<void> {
  return invoke("set_highlight_label", { id, label });
}

/// Set or clear a passage's note (RFC 0061).
export function setHighlightNote(id: string, note: string | null): Promise<void> {
  return invoke("set_highlight_note", { id, note });
}

export function removeHighlight(id: string): Promise<void> {
  return invoke("remove_highlight", { id });
}

/// Delete a passage outright — the mark, its note, and its conversation
/// (RFC 0079 R1.3). Resolves to the number of threads that went with it, so
/// the caller can say what it cost. `removeHighlight` above deletes only the
/// mark and leaves an unreachable thread behind; prefer this one.
export function removeAnnotation(id: string): Promise<number> {
  return invoke<number>("remove_annotation", { id });
}

export function listHighlights(paperId: string): Promise<Highlight[]> {
  return invoke<Highlight[]>("list_highlights", { paperId });
}

/// Create a highlight authored by the agent (RFC 0059 Phase 2): same shape as
/// `createHighlight`, plus the model that proposed it.
export function createAgentHighlight(args: {
  paperId: string; locator: Locator; excerpt: string; color: HighlightColor; label?: string | null; model: string;
}): Promise<Highlight> {
  return invoke<Highlight>("create_agent_highlight", {
    paperId: args.paperId, locator: args.locator, excerpt: args.excerpt,
    color: args.color, label: args.label ?? null, model: args.model,
  });
}

export function listAgentHighlights(paperId: string): Promise<Highlight[]> {
  return invoke<Highlight[]>("list_agent_highlights", { paperId });
}
