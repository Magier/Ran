import { getContext, setContext } from 'svelte';
import type { ArmoryType, Node } from '$lib/model';
import type {
	AttackFlow,
	CampaignState as State,
	Graph,
	TTP,
	ExecuteActionRequest
} from '$lib/api/index';
import type { KubetierCatalog } from '$lib/api/index';
import { showToast, type ToastType } from '$lib/components/toaster';
import { getRanAPI, RanAPI } from '$lib/ran_api';
import { timeline } from '$lib/stores/timelineStore.svelte';

// Great video how to build stores in Svelte 5: https://www.youtube.com/watch?v=kMBDsyozllk

type Conditions = {};

export type Entity = {
	id: string;
	name: string;
	kind?: string;
	namespace?: string;
	accessLevel?: { User: number; Level: number } | string;
	binaries?: Record<string, string>;
	envVars?: Record<string, string>;
	phase?: string;
	ready?: boolean;
	stateReason?: string;
	ips?: string[];
	provenance?: string[];
};

export type Relation = {
	id: string;
	source: string;
	destination: string;
	kind: string;
	[key: string]: any; // Allow additional fields from specific relation types
};

type ErrorMsg = {
	CmdId: string;
	Level: string;
	Msg: string;
};

type BackendError = {
	code: string;
	message: string;
};

type ParseAuditUI = {
	effectId: string;
	parseResult: string;
	detail: string;
	inferredFactsWritten: number;
};

function normalizeParseAudit(raw: any): ParseAuditUI {
	return {
		effectId: raw?.effectId ?? raw?.effect_id ?? 'unknown effect',
		parseResult: raw?.parseResult ?? raw?.parse_result ?? 'UnknownFormat',
		detail: raw?.detail ?? 'no additional parser detail',
		inferredFactsWritten: raw?.inferredFactsWritten ?? raw?.inferred_facts_written ?? 0
	};
}

class CampaignState {
	campaignId: number = $state(0);
	activeConditions: Conditions = $state({});
	entities = $state<Entity[]>([]);
	relations = $state<Map<string, Relation>>(new Map());
	namespaces = $state<Entity[]>([]);
	pods = $state<Entity[]>([]);
	serviceAccounts = $state<Entity[]>([]);
	armory = $state<ArmoryType>(new Map());
	graph = $state<Graph>({} as Graph);
	kubetier = $state<KubetierCatalog | null>(null);
	/// Bumped whenever the scoring profile changes, so recommendation views refetch.
	scoringVersion = $state(0);
	pendingMessages: string[] = [];
	api: RanAPI = $state(getRanAPI());
	private factsChangedCounter = 0;
	private getCampaignStateCounter = 0;

	init(url?: string): Promise<void> {
		// If no URL provided, it will auto-construct from window.location

		this.api.on('armory-loaded', (data) => {
			this.armory = parseArmory(data);
		});
		this.api.on('facts-changed', (data: any) => {
			const eventId = ++this.factsChangedCounter;
			this.api.GetGraph().then((g: Graph) => {
				this.graph = g;
			});

			const stateCallId = ++this.getCampaignStateCounter;
			this.api
				.GetCampaignState()
				.then((s: State) => {
					this.#setState(s);
				})
				.catch((err) => {
					console.error(`❌ [Event ${eventId}->Call ${stateCallId}] GetCampaignState failed:`, err);
				});
		});
		this.api.on('parse-audited', (data: any) => {
			const audits = (data?.audits ?? []).map(normalizeParseAudit);
			if (!Array.isArray(audits) || audits.length === 0) {
				showToast('Parsing coverage', 'No parse audits were emitted for this action', 'error');
				return;
			}

			const logOnly = new Set(['NoParser', 'KnownFailure']);
			const problematic = audits.filter(
				(a: ParseAuditUI) => a.parseResult !== 'Parsed' && !logOnly.has(a.parseResult)
			);
			const gaps = audits.filter((a: ParseAuditUI) => logOnly.has(a.parseResult));
			if (gaps.length > 0) {
				console.log(
					'[parse-audited] parser gaps (log only):',
					gaps.map((a: ParseAuditUI) => `${a.effectId}: ${a.parseResult} (${a.detail})`)
				);
			}
			if (problematic.length > 0) {
				const details = problematic
					.map((a: ParseAuditUI) => `${a.effectId}: ${a.parseResult} (${a.detail})`)
					.join('\n');
				showToast('Parsing gap detected', details, 'error');
				return;
			}
		});
		this.api.on('reset-campaign', () => this.onReset());
		this.api.on('error-msg', (rawMsg: string) => {
			let msg: ErrorMsg = JSON.parse(rawMsg);

			// Map msg.Level to ToastType
			let toastType: ToastType;
			switch (msg.Level) {
				case 'ERROR':
				case 'WARN':
				case 'FATAL':
					toastType = 'error';
					break;
				case 'INFO':
				case 'DEBUG':
				default:
					toastType = 'info';
			}

			showToast('Error', msg.Msg, toastType);
		});
		console.log('CampaignState connecting to backend...');
		return this.api.connect().then((a) => {
			this.api
				.GetKubetierCatalog()
				.then((catalog) => {
					this.kubetier = catalog;
				})
				.catch((err) => console.warn('Failed to load offline KubeTier catalog', err));
			this.api.GetGraph().then((g: Graph) => {
				this.graph = g;
			});
			this.api.GetCampaignState().then((s: State) => {
				this.#setState(s);
			});
			this.api.GetArmory().then((a: TTP[]) => {
				this.armory = parseArmory(a);
			});
		});
	}

	isReady(): boolean {
		return this.graph && this.entities.length > 0;
	}

	showError(msg: string | object) {
		if (typeof msg === 'object') {
			if (msg.hasOwnProperty('message')) {
				msg = (msg as any).message;
			} else {
				// fallback handling to show full object (may allow later refinement)
				msg = JSON.stringify(msg);
			}
		} else if (typeof msg !== 'string') {
			msg = String(msg);
		}

		console.error(msg);
		showToast('Error', JSON.stringify(msg), 'error');
	}

	reset() {
		this.entities = [];
		this.namespaces = [];
		this.pods = [];
		this.serviceAccounts = [];
		this.campaignId += 1; // Increment campaign ID, to trigger changes based on new campaign
		this.api.ResetCampaign().then(() => {
			this.api.GetGraph().then((g: Graph) => {
				this.graph = g;
			});
		});
	}

	async onReset(): Promise<void> {
		console.log('Received reset-campaign event from backend');
		// Clear the in-memory operation timeline so a reset/restart from any path
		// (backend restart, autonomous loop, CLI) doesn't carry previous
		// iterations' logs into the new campaign. The app-menu Reset clears it
		// directly; this covers every reset signalled via the SSE event. Sharing
		// the single reset-campaign handler is required — ran_api's `on()` keeps
		// one handler per event type, so a second listener would clobber this one.
		timeline.clear();
		await this.api.GetGraph().then((g: Graph) => {
			this.graph = g;
		});
		await this.api.GetCampaignState().then((s: State) => {
			this.#setState(s);
		});
	}

	#setState(state: State): void {
		let entities = [];
		let namespaces = [];
		let pods = [];
		let serviceAccounts = [];

		const timestamp = new Date().toISOString();
		for (const [id, entity] of Object.entries(state.entities || {})) {
			const typedEntity = entity as unknown as Entity;
			if (typedEntity === null) {
				console.warn(`⚠️ Skipping entity with id ${id} because it is null`);
				console.log(state.entities);
				continue;
			}

			if (typedEntity.kind === 'Namespace') {
				namespaces.push(typedEntity);
			} else if (typedEntity.kind === 'Pod') {
				pods.push(typedEntity);
			} else if (typedEntity.kind === 'ServiceAccount') {
				serviceAccounts.push(typedEntity);
			}

			if (!typedEntity.id) {
				typedEntity.id = id;
			}
			entities.push(typedEntity);
		}

		this.entities = entities;
		this.namespaces = namespaces;
		this.pods = pods;
		this.serviceAccounts = serviceAccounts;

		// Process relations
		console.info('Setting campaign state with relations:', state.relations);
		const relationsMap = new Map<string, Relation>();
		for (const relation of state.relations || []) {
			// Extract source and target from the relation
			// The backend sends relations with all their fields
			const id = relation.id as string;
			if (id) {
				relationsMap.set(id, relation as Relation);
			}
		}
		this.relations = relationsMap;
		timeline.backfillBootstrap(state.bootstrapOperations ?? []);
	}

	// #updateState is currently unused but kept for potential future use
	// #updateState(state: State): void {
	// 	for (const [id, entity] of Object.entries(state.entities || {})) {
	// 		const typedEntity = entity as any as Entity;
	// 		if (!this.entities.some((e) => e.id === typedEntity.id)) {
	// 			this.entities = [...this.entities, typedEntity];
	// 		} else {
	// 			// TODO: properly update the entity
	// 		}
	// 		if (typedEntity.kind === 'Namespace') {
	// 			this.namespaces = [...this.namespaces, typedEntity];
	// 		}
	// 	}
	// }

	// updateEntities(data: FactsChanged): Entity[] {
	//     // Update the entities based on the facts changed
	//     for (const entity of data.RemovedEntities || []) {
	//         this.entities = this.entities.filter(e => e.id !== entity.id);
	//     }

	//     for (const entity of data.NewEntities || []) {
	//         if (!this.entities.some(e => e.id === entity.id)) {
	//             console.log("Adding entity: ", entity);
	//             this.entities = [...this.entities, entity];
	//         } else {
	//             // TODO: properly update the entity
	//         }
	//         if (entity.kind === 'Namespace') {
	//             this.namespaces = [...this.namespaces, entity];
	//         }
	//     }
	//     return this.entities
	// }

	getTtpById(id: string): TTP | undefined {
		for (const [group, ttps] of this.armory) {
			const ttp = ttps.find((t) => t.id === id);
			if (ttp) {
				return ttp;
			}
		}
	}

	getNamespaces(): Entity[] {
		let ns = this.entities.filter((entity) => entity.kind === 'Namespace');
		return ns || [];
	}

	getPods(ns?: string): Entity[] {
		let pods = this.entities.filter(
			(entity) => entity.kind === 'Pod' && (!ns || entity.namespace === ns)
		);
		return pods || [];
	}

	getObjectById(id: string): Entity | Relation | undefined {
		if (isRelation(id)) {
			return this.getRelationById(id);
		} else {
			return this.getEntityById(id);
		}
	}

	getEntityById(id: string): Entity | undefined {
		if (id === '') {
			return undefined;
		}
		const found = this.entities.find((entity) => entity.id === id);
		if (!found) {
			console.warn(`❌ Entity not found for id: ${id}, available entities:`);
		}
		return found;
	}

	getRelationById(id: string): Relation | undefined {
		if (id === '') {
			return undefined;
		}

		// First check if we have the full relation data stored
		const storedRelation = this.relations.get(id);
		if (storedRelation) {
			return storedRelation;
		}

		// Fallback: parse the ID to create a basic relation object
		const parts = id.match(/^(.+?)-\[(.+?)\]->(.+)$/);
		if (parts) {
			const [, source, label, destination] = parts;
			return {
				id: id,
				source: source,
				destination: destination,
				kind: label
			};
		}
		return undefined;
	}

	getCompromisedSystems(): Entity[] {
		const systemKinds = ['Pod', 'K8sNode', 'UnknownSystem'];
		return this.entities.filter(
			(entity) =>
				systemKinds.includes(entity.kind ?? '') &&
				typeof entity.accessLevel === 'string' &&
				entity.accessLevel.endsWith('exec')
		);
	}

	getServiceAccounts(ns?: string, permissions?: string[], includeUnkwnon?: boolean): Entity[] {
		let serviceAccounts = this.entities.filter((entity) => entity.kind === 'ServiceAccount');
		if (ns) {
			serviceAccounts = serviceAccounts.filter((entity) => entity.namespace === ns);
		}
		return serviceAccounts || [];
	}

	getServiceAccountsWithTokens(ns?: string): Entity[] {
		// Get all service account tokens
		const tokens = this.entities.filter(
			(entity) =>
				entity.kind === 'ServiceAccountToken' ||
				(entity.kind === 'ServiceAccount' && entity.hasOwnProperty('token'))
		); // Include ServiceAccounts that have token binaries

		// Extract the ServiceAccount IDs from tokens (tokens have ID format: ns/{namespace}/sa/{saName}/token)
		const saIdsWithTokens = new Set(
			tokens.map((token) => {
				// Extract SA ID from token ID by removing the /token suffix
				const tokenId = token.id;
				return tokenId.replace(/\/token$/, '');
			})
		);

		// Filter ServiceAccounts to only those that have tokens
		let serviceAccounts = this.entities.filter(
			(entity) => entity.kind === 'ServiceAccount' && saIdsWithTokens.has(entity.id)
		);

		if (ns) {
			serviceAccounts = serviceAccounts.filter((entity) => entity.namespace === ns);
		}

		return serviceAccounts || [];
	}

	// ExecuteAction(
	// 	actionId: string,
	// 	targetId: string,
	// 	procedureId: string,
	// 	args: Record<string, any>
	// ): Promise<void> {
	// 	const cmd: ExecuteActionRequest = {actionId, targetId, procedureId, args};
	// 	return this.api.ExecuteAction(cmd).then(() => {});
	// }

	GetFlow(): Promise<AttackFlow> {
		return this.api.GetFlow();
	}
}

const DEFAULT_KEY = '$_campaignState';

export const getCampaignState = (key = DEFAULT_KEY) => {
	return getContext<CampaignState>(key);
};

export const setCampaignState = (key = DEFAULT_KEY) => {
	const campaignState = new CampaignState();
	return setContext(key, campaignState);
};

export function parseArmory(data: TTP[]): ArmoryType {
	// this comes from the backend must be converted
	let armoryMap = new Map<string, TTP[]>();
	for (let ttp of data) {
		let groupName = ttp.tactic;
		if (groupName === '') {
			groupName = 'Other';
		}
		if (!armoryMap.has(groupName)) {
			armoryMap.set(groupName, []);
		}
		armoryMap.get(groupName)!.push(ttp);
	}
	// Armory contains a CmdId field; process accordingly if needed.
	return armoryMap;
}
export function isRelation(id: string): boolean {
	return id.indexOf('->') !== -1;
}
