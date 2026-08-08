import { invoke } from "@tauri-apps/api/core";
import type { ContextItem, ContextItemView, ContextKey } from "$lib/domain/context";
import type { ChatThreadView } from "$lib/domain/chat";

/**
 * Commit a chunk to a thread's persistent context (RFC 0077).
 *
 * Idempotent: adding a chunk already in context returns the existing item.
 */
export async function addChatContext(threadId: string, chunkId: string): Promise<ContextItem> {
  return invoke<ContextItem>("add_chat_context", { threadId, chunkId });
}

/** Drop one item, by item id or chunk id — whichever the caller holds. */
export async function deleteChatContext(threadId: string, key: ContextKey): Promise<boolean> {
  return invoke<boolean>("delete_chat_context", { threadId, key });
}

/**
 * A thread's persistent context, resolved to text.
 *
 * Items whose chunk no longer resolves come back with `unresolved: true`
 * rather than missing — a silently shrinking context is worse than a visible
 * hole.
 */
export async function listChatContext(threadId: string): Promise<ContextItemView[]> {
  return invoke<ContextItemView[]>("list_chat_context", { threadId });
}

/**
 * Summarize the thread's context and set a watermark.
 *
 * The one context call that costs a model round-trip. Chat entries are not
 * touched: the thread keeps showing every turn, only the next prompt shrinks.
 */
export async function compactChatContext(threadId: string): Promise<ChatThreadView> {
  return invoke<ChatThreadView>("compact_chat_context", { threadId });
}
