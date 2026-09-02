import { invoke } from "@tauri-apps/api/core";
import type {
  HarnessImprovement,
  HarnessImprovementStatus,
  HarnessImprovementValue,
} from "$lib/domain/harness-improvement";

export async function listHarnessImprovements(
  projectId: string,
  status?: HarnessImprovementStatus,
): Promise<HarnessImprovement[]> {
  return invoke<HarnessImprovement[]>("list_harness_improvements", { projectId, status });
}

export async function getHarnessImprovement(improvementId: string): Promise<HarnessImprovement> {
  return invoke<HarnessImprovement>("get_harness_improvement", { improvementId });
}

export async function editHarnessImprovement(
  improvementId: string,
  proposedValue: HarnessImprovementValue,
): Promise<HarnessImprovement> {
  return invoke<HarnessImprovement>("edit_harness_improvement", {
    improvementId,
    proposedValue,
  });
}

export async function acceptHarnessImprovement(improvementId: string): Promise<HarnessImprovement> {
  return invoke<HarnessImprovement>("accept_harness_improvement", { improvementId });
}

export async function rejectHarnessImprovement(
  improvementId: string,
  reason?: string,
): Promise<HarnessImprovement> {
  return invoke<HarnessImprovement>("reject_harness_improvement", { improvementId, reason });
}
