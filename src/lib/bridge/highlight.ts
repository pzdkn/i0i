import { invoke } from "@tauri-apps/api/core";
import type { Highlight, HighlightColor, Locator } from "$lib/domain/highlight";

export function createHighlight(args: {
  paperId: string; locator: Locator; excerpt: string; color: HighlightColor; label?: string | null;
}): Promise<Highlight> {
  return invoke<Highlight>("create_highlight", {
    paperId: args.paperId, locator: args.locator, excerpt: args.excerpt,
    color: args.color, label: args.label ?? null,
  });
}

export function recolorHighlight(id: string, color: HighlightColor): Promise<void> {
  return invoke("recolor_highlight", { id, color });
}

export function setHighlightLabel(id: string, label: string | null): Promise<void> {
  return invoke("set_highlight_label", { id, label });
}

export function removeHighlight(id: string): Promise<void> {
  return invoke("remove_highlight", { id });
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
