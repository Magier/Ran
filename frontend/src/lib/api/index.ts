/**
 * Simplified type exports from OpenAPI spec
 * Auto-generated - do not edit manually
 * Run: make generate-api
 */

import type { components, paths, operations } from './gen_types';

// Schema types with simple names
export type Graph = components['schemas']['Graph'];
export type Node = components['schemas']['Node'];
export type Edge = components['schemas']['Edge'];
export type CampaignState = components['schemas']['CampaignState'];
export type AttackFlow = components['schemas']['AttackFlow'];
export type AttackStep = components['schemas']['AttackStep'];
export type TraversalHop = components['schemas']['TraversalHop'];
export type TTP = components['schemas']['TTP'];
export type TTPParam = components['schemas']['TTPParam'];
export type TTPDefense = components['schemas']['TTPDefense'];
export type SigmaRule = components['schemas']['SigmaRule'];
export type ExecuteActionCmd = components['schemas']['ExecuteActionCmd'];
export type AuthIdentity = components['schemas']['AuthIdentity'];
export type ExecutionRecordEntry = components['schemas']['ExecutionRecordEntry'];
export type K8sResource = components['schemas']['K8sResource'];
export type ApiError = components['schemas']['Error'];

export type ScoredCandidate = components['schemas']['ScoredCandidate'];
export type ConsiderationScore = components['schemas']['ConsiderationScore'];
export type ScoringProfile = components['schemas']['ScoringProfile'];
export type ScoringProfileUpdate = components['schemas']['ScoringProfileUpdate'];
export type NamedConsideration = components['schemas']['NamedConsideration'];
export type ResponseCurve = components['schemas']['ResponseCurve'];
export type CombinationMode = components['schemas']['CombinationMode'];
export type CalibrationResult = components['schemas']['CalibrationResult'];
export type CalibrationMetrics = components['schemas']['CalibrationMetrics'];

export type RBACPermission = components['schemas']['RBACPermission'];

export type PlanSummary = components['schemas']['PlanSummary'];
export type LoadPlanRequest = components['schemas']['LoadPlanRequest'];

// Operation types for request/response
export type GetGraphResponse =
	operations['getGraph']['responses']['200']['content']['application/json'];
export type GetCampaignStateResponse =
	operations['getCampaignState']['responses']['200']['content']['application/json'];
export type GetArmoryResponse =
	operations['getArmory']['responses']['200']['content']['application/json'];
export type GetApplicableTTPsResponse =
	operations['getApplicableTTPs']['responses']['200']['content']['application/json'];
export type GetFlowResponse =
	operations['getFlow']['responses']['200']['content']['application/json'];
export type ExecuteActionRequest =
	operations['executeAction']['requestBody']['content']['application/json'];
export type ExecuteActionResponse =
	operations['executeAction']['responses']['200']['content']['application/json'];
export type SaveFlowRequest = operations['saveFlow']['requestBody']['content']['application/json'];
export type SaveFlowResponse =
	operations['saveFlow']['responses']['200']['content']['application/json'];

// Re-export original types for advanced usage
export type { components, paths, operations };
