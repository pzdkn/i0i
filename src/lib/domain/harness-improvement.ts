export type HarnessImprovementStatus = "proposed" | "accepted" | "rejected" | "superseded";
export type HarnessImprovementTarget =
  | "preferred_concepts"
  | "excluded_concepts"
  | "metadata_resolvers";
export type HarnessObservationKind =
  | "query_quality"
  | "irrelevant_result_class"
  | "source_failure"
  | "terminology"
  | "coverage_bias"
  | "relevance_error"
  | "scope_drift"
  | "wasted_work";

export type HarnessImprovementValue =
  | { kind: "concepts"; items: string[] }
  | { kind: "metadata_resolvers"; items: string[] };

export interface HarnessImprovement {
  id: string;
  projectId: string;
  status: HarnessImprovementStatus;
  target: HarnessImprovementTarget;
  baseConfigurationVersion: number;
  beforeValue: HarnessImprovementValue;
  proposedValue: HarnessImprovementValue;
  rationale: string;
  expectedEffect: string;
  policyVersion: string;
  decisionActor?: string;
  decisionReason?: string;
  resultingConfigurationVersion?: number;
  runIds: string[];
  observationIds: string[];
  observations: HarnessObservation[];
  createdAt: string;
  decidedAt?: string;
}

export interface HarnessObservation {
  id: string;
  reflectionId: string;
  runId: string;
  projectId: string;
  kind: HarnessObservationKind;
  signature: string;
  severity: number;
  confidence: number;
  description: string;
  metricsJson: string;
  target?: HarnessImprovementTarget;
  proposedValue?: HarnessImprovementValue;
  proposalEligible: boolean;
  createdAt: string;
}
