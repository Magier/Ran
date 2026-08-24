/**
 * RanAPI - Client library for Ran API
 *
 * Real-time events use Server-Sent Events (SSE). Commands and queries use REST.
 *
 * Usage:
 * ```typescript
 * import { ranAPI, connect, on } from '$lib/ran_api';
 *
 * // Connect to the SSE event stream
 * await connect();
 *
 * // Subscribe to events
 * on('armory-loaded', (data) => console.log('Armory:', data));
 * on('ttp-executed', (data) => console.log('TTP executed:', data));
 *
 * // Use REST API methods
 * const graph = await ranAPI.GetGraph();
 * await ranAPI.ExecuteAction({ actionId: '...', targetId: '...' });
 * ```
 */
import createClient from 'openapi-fetch';
import type { paths } from '$lib/api/gen_types';
import type {
	Graph,
	CampaignState,
	TTP,
	AttackFlow,
	ExecuteActionCmd,
	ExecutionRecordEntry,
	K8sResource,
	ScoredCandidate,
	ScoringProfile,
	ScoringProfileUpdate,
	CalibrationResult,
	PlanSummary,
	KubetierCatalog
} from '$lib/api';

export class RanAPI {
	eventSource?: EventSource;
	private messageHandlers = new Map<string, (data: any) => void>();
	private sseEventListeners = new Set<string>(); // Track registered SSE event types
	private pendingSSEEventTypes = new Set<string>(); // Event types waiting for SSE connection
	private restClient = createClient<paths>({ baseUrl: '' }); // Use relative URLs

	connect(url?: string): Promise<void> {
		return this.connectSSE(url);
	}

	private connectSSE(url?: string): Promise<void> {
		// Construct SSE URL from current location if not provided
		if (!url) {
			url = `/events`;
		}
		console.info('Connecting to SSE at', url);
		return new Promise((resolve, reject) => {
			this.eventSource = new EventSource(url);

			this.eventSource.onopen = () => {
				console.log('SSE connection established');

				// Register any event listeners that were added before connection was ready
				this.pendingSSEEventTypes.forEach((type) => {
					if (!this.sseEventListeners.has(type) && this.eventSource) {
						this.sseEventListeners.add(type);
						this.eventSource.addEventListener(type, (event: MessageEvent) => {
							this.handleSSEMessage(event);
						});
					}
				});
				this.pendingSSEEventTypes.clear();

				resolve();
			};

			this.eventSource.onmessage = (event) => {
				console.info('Received SSE message:', event.data);
				this.handleSSEMessage(event);
			};

			this.eventSource.onerror = (err) => {
				console.error('SSE error:', err);
				// SSE automatically reconnects, so only reject if not yet connected
				if (this.eventSource?.readyState === EventSource.CONNECTING) {
					reject(err);
				}
			};

			// Note: We don't set onmessage here because SSE events from the backend
			// use named events (event: <type>), which only trigger addEventListener
			// Event listeners are registered dynamically in the on() method
		});
	}

	private handleSSEMessage(event: MessageEvent) {
		let msgType: string;
		let data: any;
		try {
			({ type: msgType, data } = JSON.parse(event.data));
		} catch (err) {
			console.error('Failed to parse SSE message:', err, event.data);
			return;
		}

		// Call registered handler for events
		const handler = this.messageHandlers.get(msgType);
		if (handler) {
			try {
				handler(data);
			} catch (err) {
				console.error('Error in SSE message handler for type:', msgType, err);
			}
		} else {
			console.debug('Unhandled SSE event type:', msgType);
		}
	}

	// Subscribe to push events (events not triggered by a request)
	on(type: string, handler: (data: any) => void) {
		this.messageHandlers.set(type, handler);
		console.log(`Registered handler for event type: ${type}`);

		if (
			this.eventSource &&
			this.eventSource.readyState === EventSource.OPEN &&
			!this.sseEventListeners.has(type)
		) {
			// Connection is ready and open, register immediately
			this.sseEventListeners.add(type);
			this.eventSource.addEventListener(type, (event: MessageEvent) => {
				this.handleSSEMessage(event);
			});
		} else {
			// Connection not ready yet or already registered, add to pending to ensure it gets registered
			if (!this.sseEventListeners.has(type)) {
				this.pendingSSEEventTypes.add(type);
				console.log(`SSE event listener queued for type: ${type} (will register on connection)`);
			}
		}
	}

	off(type: string) {
		this.messageHandlers.delete(type);

		// Note: EventSource doesn't provide a way to remove specific event listeners
		// The handler just won't be called anymore since we removed it from messageHandlers
	}

	disconnect() {
		if (this.eventSource) {
			this.eventSource.close();
			this.eventSource = undefined;
		}
		this.messageHandlers.clear();
		this.sseEventListeners.clear();
		this.pendingSSEEventTypes.clear();
	}

	// REST API Methods (auto-generated from OpenAPI spec)
	async GetGraph(): Promise<Graph> {
		const { data, error } = await this.restClient.GET('/api/graph');
		if (error) throw new Error('Failed to get graph');
		return data;
	}

	async GetCampaignState(): Promise<CampaignState> {
		const { data, error } = await this.restClient.GET('/api/campaign-state');
		if (error) throw new Error('Failed to get campaign state');
		return data;
	}

	async GetKubetierCatalog(): Promise<KubetierCatalog> {
		const { data, error } = await this.restClient.GET('/api/kubetier');
		if (error) throw new Error('Failed to get offline KubeTier catalog');
		return data;
	}

	async GetArmory(): Promise<Array<TTP>> {
		const { data, error } = await this.restClient.GET('/api/armory');
		if (error) throw new Error('Failed to get armory');
		return data;
	}

	async GetApplicableTTPs(targetId: string): Promise<Array<TTP>> {
		if (!targetId) {
			console.warn('GetApplicableTTPs called with empty targetId');
			return [];
		}

		const { data, error } = await this.restClient.GET('/api/applicable-ttps', {
			params: { query: { targetId } }
		});
		console.log('Applicable TTPs data:', data, 'error:', error, 'for targetId:', targetId);
		if (error) {
			console.warn('Failed to get applicable TTPs', error);
			return [];
		}
		return data;
	}

	async GetEligibleAuthIdentities(actionId: string, targetId: string) {
		const { data, error } = await this.restClient.GET('/api/eligible-auth-identities', {
			params: { query: { actionId, targetId } }
		});
		if (error) throw new Error(error.error || 'Failed to get eligible authentication identities');
		return data;
	}

	async GetRecommendations(targetId?: string, limit?: number): Promise<Array<ScoredCandidate>> {
		const query: { targetId?: string; limit?: number } = {};
		if (targetId) query.targetId = targetId;
		if (limit !== undefined) query.limit = limit;

		const { data, error } = await this.restClient.GET('/api/recommendations', {
			params: { query }
		});
		if (error) {
			console.warn('Failed to get recommendations', error);
			return [];
		}
		return data;
	}

	async GetScoringProfile(): Promise<ScoringProfile | null> {
		const { data, error } = await this.restClient.GET('/api/scoring/profile');
		if (error) {
			console.warn('Failed to get scoring profile', error);
			return null;
		}
		return data;
	}

	async UpdateScoringProfile(update: ScoringProfileUpdate): Promise<ScoringProfile | null> {
		const { data, error } = await this.restClient.PUT('/api/scoring/profile', {
			body: update
		});
		if (error) {
			console.warn('Failed to update scoring profile', error);
			return null;
		}
		return data;
	}

	async SaveScoringProfile(): Promise<ScoringProfile | null> {
		const { data, error } = await this.restClient.POST('/api/scoring/profile/save');
		if (error) {
			console.warn('Failed to save scoring profile', error);
			return null;
		}
		return data;
	}

	async ResetScoringProfile(): Promise<ScoringProfile | null> {
		const { data, error } = await this.restClient.POST('/api/scoring/profile/reset');
		if (error) {
			console.warn('Failed to reset scoring profile', error);
			return null;
		}
		return data;
	}

	/** Fit a profile from captured operator decisions. Returns the preview +
	 * metrics, or null if calibration isn't possible yet (no decisions / disabled). */
	async CalibrateScoring(): Promise<CalibrationResult | null> {
		const { data, error } = await this.restClient.POST('/api/scoring/calibrate');
		if (error) {
			console.warn('Failed to calibrate scoring profile', error);
			return null;
		}
		return data;
	}

	async GetFlow(): Promise<AttackFlow> {
		const { data, error } = await this.restClient.GET('/api/flow');
		if (error) throw new Error('Failed to get flow');
		return data;
	}

	async GetExecutionRecords(): Promise<ExecutionRecordEntry[]> {
		const { data, error } = await this.restClient.GET('/api/execution-records');
		if (error) throw new Error('Failed to get execution records');
		return data;
	}

	async ExecuteAction(cmd: ExecuteActionCmd) {
		const { data, error } = await this.restClient.POST('/api/action/execute', {
			body: cmd
		});
		if (error) throw new Error(error.error || 'Failed to execute action');
		return data;
	}

	async ResetCampaign() {
		const { data, error } = await this.restClient.POST('/api/campaign/reset');
		if (error) throw new Error('Failed to reset campaign');
		return data;
	}

	async GetRunningPods(namespace?: string): Promise<Array<K8sResource>> {
		const { data, error } = await this.restClient.GET('/api/pods/running', {
			params: { query: { namespace } }
		});
		if (error) throw new Error(error.error);
		return data;
	}

	async GetFileContent(path: string): Promise<{ path?: string; content?: string }> {
		const { data, error } = await this.restClient.GET('/api/files', {
			params: { query: { path } }
		});
		if (error) throw new Error(error.error);
		return data;
	}

	async ListPlans(): Promise<Array<PlanSummary>> {
		const { data, error } = await this.restClient.GET('/api/plans/available');
		if (error) throw new Error(error.error || 'Failed to list plans');
		return data;
	}

	async LoadPlan(filename: string): Promise<{ plan_id?: string }> {
		const { data, error } = await this.restClient.POST('/api/plans/load', {
			body: { filename }
		});
		if (error) throw new Error(error.error || 'Failed to load plan');
		return data;
	}

	async ExportPlan(includeFailed = false, name?: string, description?: string): Promise<string> {
		const { data, error } = await this.restClient.GET('/api/plans/export', {
			params: { query: { include_failed: includeFailed, name, description } },
			parseAs: 'text'
		});
		if (error) throw new Error('Failed to export plan');
		return data;
	}
}

// Singleton instance
const ranAPI = new RanAPI();

export function getRanAPI(): RanAPI {
	return ranAPI;
}

// Export the singleton
export { ranAPI };

// Export bound functions for convenience
export const connect = ranAPI.connect.bind(ranAPI);
export const disconnect = ranAPI.disconnect.bind(ranAPI);
export const on = ranAPI.on.bind(ranAPI);
export const off = ranAPI.off.bind(ranAPI);
export const GetGraph = ranAPI.GetGraph.bind(ranAPI);
export const GetCampaignState = ranAPI.GetCampaignState.bind(ranAPI);
export const GetKubetierCatalog = ranAPI.GetKubetierCatalog.bind(ranAPI);
export const GetArmory = ranAPI.GetArmory.bind(ranAPI);
export const GetApplicableTTPs = ranAPI.GetApplicableTTPs.bind(ranAPI);
export const GetRecommendations = ranAPI.GetRecommendations.bind(ranAPI);
export const GetFlow = ranAPI.GetFlow.bind(ranAPI);
export const ExecuteAction = ranAPI.ExecuteAction.bind(ranAPI);
export const ResetCampaign = ranAPI.ResetCampaign.bind(ranAPI);
export const GetRunningPods = ranAPI.GetRunningPods.bind(ranAPI);
export const ListPlans = ranAPI.ListPlans.bind(ranAPI);
export const LoadPlan = ranAPI.LoadPlan.bind(ranAPI);
