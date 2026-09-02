import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  CreateFromResearchRequest,
  ResearchDocumentGeneration,
} from "$lib/domain/research-document";

export function createDocumentFromResearch(
  request: CreateFromResearchRequest,
): Promise<ResearchDocumentGeneration> {
  return invoke("create_document_from_research", { request });
}

export function getResearchDocumentGeneration(
  generationId: string,
): Promise<ResearchDocumentGeneration> {
  return invoke("get_research_document_generation", { generationId });
}

export function cancelResearchDocumentGeneration(
  generationId: string,
): Promise<ResearchDocumentGeneration> {
  return invoke("cancel_research_document_generation", { generationId });
}

export function retryResearchDocumentGeneration(
  generationId: string,
): Promise<ResearchDocumentGeneration> {
  return invoke("retry_research_document_generation", { generationId });
}

export function listenResearchDocumentGenerationUpdated(
  listener: (generation: ResearchDocumentGeneration) => void,
): Promise<UnlistenFn> {
  return listen<ResearchDocumentGeneration>(
    "research_document_generation_updated",
    (event) => listener(event.payload),
  );
}
