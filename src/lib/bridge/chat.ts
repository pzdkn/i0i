import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  ChatScope,
  ChatProgress,
  ChatThreadSummary,
  ChatThreadView,
  PinnedHighlight,
  ThreadAnchor,
} from "$lib/domain/chat";

export async function listChatThreads(scope: ChatScope): Promise<ChatThreadSummary[]> {
  return invoke<ChatThreadSummary[]>("list_chat_threads", { scope });
}

export async function getChatThread(threadId: string): Promise<ChatThreadView> {
  return invoke<ChatThreadView>("get_chat_thread", { threadId });
}

/// Add a note at an anchor; the thread is created lazily and returned (RFC 0034).
export async function noteAtAnchor(
  scope: ChatScope,
  anchor: ThreadAnchor,
  body: string,
): Promise<ChatThreadView> {
  return invoke<ChatThreadView>("add_note_at_anchor", { scope, anchor, body });
}

export async function addChatNote(threadId: string, body: string): Promise<ChatThreadView> {
  return invoke<ChatThreadView>("add_chat_note", { threadId, body });
}

export async function setChatEntryPinned(entryId: string, pinned: boolean): Promise<void> {
  return invoke<void>("set_chat_entry_pinned", { entryId, pinned });
}

export async function listPinnedChatEntries(scope: ChatScope): Promise<PinnedHighlight[]> {
  return invoke<PinnedHighlight[]>("list_pinned_chat_entries", { scope });
}

export async function renameChatThread(threadId: string, title: string): Promise<void> {
  return invoke<void>("rename_chat_thread", { threadId, title });
}

export async function deleteChatThread(threadId: string): Promise<void> {
  return invoke<void>("delete_chat_thread", { threadId });
}

type ChatStreamEvent =
  | { event: "progress"; progress: ChatProgress }
  | { event: "delta"; text: string }
  | { event: "done"; thread: ChatThreadView }
  | { event: "error"; message: string };

type AnnotateEvent =
  | { event: "intent"; quote: string; color: string; label: string | null; note: string | null }
  | { event: "done" }
  | { event: "error"; message: string };

/// A model-proposed highlight (RFC 0059): a verbatim quote to locate in the
/// reader plus the color/label/note the model attached. `color` arrives already
/// normalized to a palette color by the backend.
export type HighlightIntent = {
  quote: string;
  color: string;
  label: string | null;
  note: string | null;
};

/// Ask in a thread, invoking `onDelta` for each streamed fragment, and resolve
/// with the updated thread once the reply completes.
export async function askChatThreadStreamed(
  threadId: string,
  body: string,
  onDelta: (text: string) => void,
  onProgress: (progress: ChatProgress) => void = () => {},
): Promise<ChatThreadView> {
  return streamAsk("ask_chat_thread_streamed", { threadId, body }, onDelta, onProgress);
}

/// Ask at an anchor; the thread is created lazily on success and returned
/// (RFC 0034). Prose only — marking is a separate fast-model pass
/// (`annotateStreamed`), so the answer is never slowed by tool-calling.
/**
 * Ask at an anchor, creating the thread only once the reply lands.
 *
 * `newThread` forces a fresh conversation instead of appending to this paper's
 * existing whole-paper thread — what "Ask about this paper" wants, since a new
 * question is usually a new subject.
 */
export async function askAtAnchorStreamed(
  scope: ChatScope,
  anchor: ThreadAnchor,
  body: string,
  onDelta: (text: string) => void,
  newThread = false,
  onProgress: (progress: ChatProgress) => void = () => {},
): Promise<ChatThreadView> {
  return streamAsk("ask_at_anchor_streamed", { scope, anchor, body, newThread }, onDelta, onProgress);
}

export type LogLevel = "error" | "warn" | "info" | "debug" | "trace";

/// Log a line into the backend terminal at a level (default "debug"), gated by
/// the backend's I0I_LOG threshold — so frontend detail shows up in the same
/// terminal alongside backend logs.
export function debugLog(message: string, level: LogLevel = "debug"): Promise<void> {
  return invoke("debug_log", { message, level });
}

/// Fast-model annotation pass (RFC 0059 follow-up): runs the cheap annotation
/// model with only the highlight tools and invokes `onIntent` once per marked
/// passage. Persists nothing; resolves when the pass completes. Meant to run in
/// the background, in parallel with the answer.
export async function annotateStreamed(
  scope: ChatScope,
  anchor: ThreadAnchor,
  body: string,
  onIntent: (intent: HighlightIntent) => void,
): Promise<void> {
  const channel = new Channel<AnnotateEvent>();
  return new Promise<void>((resolve, reject) => {
    channel.onmessage = (message) => {
      if (message.event === "intent") {
        onIntent({ quote: message.quote, color: message.color, label: message.label, note: message.note });
      } else if (message.event === "done") {
        resolve();
      } else if (message.event === "error") {
        reject(new Error(message.message));
      }
    };
    invoke<void>("annotate_streamed", { scope, anchor, body, onEvent: channel }).catch(reject);
  });
}

/// A lens category chosen in the AI auto-highlight menu (RFC 0064): a label, a
/// palette color, and an optional free-text instruction (the "Custom…" row).
export type AutoHighlightCategory = {
  label: string;
  color: string;
  prompt: string | null;
};

/// Explicit AI auto-highlight (RFC 0064): a command, not a conversation. Returns
/// the list of passages the AI proposes to mark (structured output). The caller
/// resolves each quote and creates the highlights (reusing the RFC 0059 path).
export async function autoHighlight(
  scope: ChatScope,
  categories: AutoHighlightCategory[],
): Promise<HighlightIntent[]> {
  return invoke<HighlightIntent[]>("auto_highlight", { scope, categories });
}

/// Shared streaming-ask plumbing: open a channel, forward deltas, and resolve
/// with the thread on `done` (or reject on `error`).
function streamAsk(
  command: string,
  args: Record<string, unknown>,
  onDelta: (text: string) => void,
  onProgress: (progress: ChatProgress) => void,
): Promise<ChatThreadView> {
  const channel = new Channel<ChatStreamEvent>();

  return new Promise<ChatThreadView>((resolve, reject) => {
    channel.onmessage = (message) => {
      if (message.event === "progress") {
        onProgress(message.progress);
      } else if (message.event === "delta") {
        onDelta(message.text);
      } else if (message.event === "done") {
        resolve(message.thread);
      } else if (message.event === "error") {
        reject(new Error(message.message));
      }
    };

    invoke<void>(command, { ...args, onEvent: channel }).catch(reject);
  });
}
