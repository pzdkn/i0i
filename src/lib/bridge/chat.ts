import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  ChatScope,
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

export async function openChatDocumentThread(scope: ChatScope): Promise<ChatThreadView> {
  return invoke<ChatThreadView>("open_chat_document_thread", { scope });
}

export async function createChatThread(
  scope: ChatScope,
  anchor: ThreadAnchor,
  title?: string,
): Promise<ChatThreadView> {
  return invoke<ChatThreadView>("create_chat_thread", { scope, anchor, title });
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
  | { event: "delta"; text: string }
  | { event: "done"; thread: ChatThreadView }
  | { event: "error"; message: string };

/// Ask in a thread, invoking `onDelta` for each streamed fragment, and resolve
/// with the updated thread once the reply completes.
export async function askChatThreadStreamed(
  threadId: string,
  body: string,
  onDelta: (text: string) => void,
): Promise<ChatThreadView> {
  const channel = new Channel<ChatStreamEvent>();

  return new Promise<ChatThreadView>((resolve, reject) => {
    channel.onmessage = (message) => {
      if (message.event === "delta") {
        onDelta(message.text);
      } else if (message.event === "done") {
        resolve(message.thread);
      } else if (message.event === "error") {
        reject(new Error(message.message));
      }
    };

    invoke<void>("ask_chat_thread_streamed", { threadId, body, onEvent: channel }).catch(reject);
  });
}
