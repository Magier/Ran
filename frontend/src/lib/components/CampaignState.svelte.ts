import { getContext, setContext } from 'svelte';
import type { ArmoryType, Node } from '$lib/model';
import type { AttackFlow, CampaignState as State, Graph, TTP, ExecuteActionRequest } from '$lib/api/index';
import { showToast, type ToastType } from '$lib/components/toaster';
import { getRanAPI, RanAPI } from '$lib/ran_api';

// Great video how to build stores in Svelte 5: https://www.youtube.com/watch?v=kMBDsyozllk

type Conditions = {};

export type Entity = {
	id: string;
	name: string;
	kind?: string;
	namespace?: string;
	accessLevel?: { User: number; Level: number } | string;
	binaries?: Record<string, string>;
	phase?: string;
	ready?: boolean;
	stateReason?: string;
	ips?: string[];
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
	allPods: Entity[] = $state([]);
	pendingMessages: string[] = [];
	api: RanAPI = $state(getRanAPI());
	private factsChangedCounter = 0;
	private getCampaignStateCounter = 0;

	init(url?: string): Promise<void> {
		// this.api.onmessage = this.handleMessage;
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
			this.api.GetCampaignState().then((s: State) => {
				this.#setState(s);
			}).catch((err) => {
				console.error(`❌ [Event ${eventId}->Call ${stateCallId}] GetCampaignState failed:`, err);
			});
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
		this.api.on('pods-changed', (data: any) => {
			const pods = data?.Pods ?? data?.pods ?? [];
			this.allPods = pods.map((p: any) => ({
				id: p.id ?? p.Id,
				name: p.name ?? p.Name,
				namespace: p.namespace ?? p.Namespace,
				kind: 'Pod',
				phase: p.phase ?? p.Phase,
				ready: p.ready ?? p.Ready,
				stateReason: p.stateReason ?? p.StateReason
			}));
		});

		console.log('CampaignState connecting to backend...');
		return this.api.connect().then((a) => {
			this.api.GetGraph().then((g: Graph) => {
				this.graph = g;
			});
			this.api.GetCampaignState().then((s: State) => {
				this.#setState(s);
			});
			this.api.GetArmory().then((a: TTP[]) => {
				this.armory = parseArmory(a);
			});
			this.api
				.GetRunningPods('')
				.then((pods) => {
					this.allPods = pods;
				})
				.catch(this.showError);
		});
	}

	handleMessage(event: MessageEvent) {
		console.warn('Received legacy WebSocket message:', event.data);
		try {
			const message = JSON.parse(event.data);
			const { type, data } = message;

			console.log('Received WebSocket message:', type);
			switch (type) {
				case 'armory-loaded':
					this.armory = parseArmory(data);
					break;
				case 'facts-changed':
					this.api.GetGraph().then((g: Graph) => {
						this.graph = g;
					});
					this.api.GetCampaignState().then((s: State) => {
						this.#setState(s);
					});
					break;
				case 'error-msg':
					const msg: ErrorMsg = typeof data === 'string' ? JSON.parse(data) : data;
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
					break;
				case 'get-graph':
					this.graph = data;
					break;
				default:
					console.log('Unknown event type:', type, data);
			}
		} catch (err) {
			console.error('Failed to parse WebSocket message:', err);
		}
	}

	// connectBackend() {
	//     console.info("CampaignState connecting to backend...");
	//     runtime.EventsOn("*", (a) => {
	//         console.log(a);
	//     });
	//     runtime.EventsOn("armory-loaded", (data) => {
	//         this.armory = parseArmory(data)
	//     });
	//     runtime.EventsOn("facts-changed", (dataStr: string) => {
	//         GetGraph().then((g: Graph) => { this.graph = g; });
	//         // TODO: properly update state based on the received fact changes
	//         const data = JSON.parse(dataStr);
	//         GetCampaignState().then((s: State) => { this.#setState(s); })
	//     })
	//     runtime.EventsOn("error-msg", (rawMsg: string) => {
	//         let msg: ErrorMsg = JSON.parse(rawMsg);

	//         // Map msg.Level to ToastType
	//         let toastType: ToastType;
	//         switch (msg.Level) {
	//             case "ERROR":
	//             case "WARN":
	//             case "FATAL":
	//                 toastType = "error";
	//                 break;
	//             case "INFO":
	//             case "DEBUG":
	//             default:
	//                 toastType = "info";
	//         }

	//         showToast("Error", msg.Msg, toastType);
	//     });

	//     GetGraph().then((g: Graph) => { this.graph = g; })
	//     GetCampaignState().then((s: State) => { this.#setState(s); })
	//     GetArmory().then((a: domain.TTP[]) => { this.armory = parseArmory(a); })
	//     GetRunningPods("").then(pods => { this.allPods = pods; }).catch(this.showError);
	// }
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
				console.log(state.entities)
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
		console.info("Setting campaign state with relations:", state.relations);
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

	getPods(ns?: string, all: boolean = false): Entity[] {
		// go beyond regular campaign state and return all pods in the cluster (regardless of exploration)
		if (all) {
			return this.allPods;
		}
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
		if (id === "") {
			return undefined;
		}
		return this.entities.find((entity) => entity.id === id);
	}

	getRelationById(id: string): Relation | undefined {
		if (id === "") {
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
			(entity) => systemKinds.includes(entity.kind ?? '') &&
				entity.accessLevel != null &&
				(entity.accessLevel === "user-exec" || (typeof entity.accessLevel === "object" && (entity.accessLevel.User > 0 || entity.accessLevel.Level > 0)))
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
		const tokens = this.entities.filter((entity) => entity.kind === 'ServiceAccountToken' || (entity.kind === 'ServiceAccount' && entity.hasOwnProperty('token'))); // Include ServiceAccounts that have token binaries
		
		// Extract the ServiceAccount IDs from tokens (tokens have ID format: ns/{namespace}/sa/{saName}/token)
		const saIdsWithTokens = new Set(
			tokens.map(token => {
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
		// return this.api.sendMessage<AttackFlow>('get-flow');
		return this.api.GetFlow();
	}
	ExportAttackFlow(): Promise<AttackFlow> {
		return this.api.sendMessage<AttackFlow>('export-attack-flow');
	}

	sendMessage(type: string, data?: any): Promise<any> {
		return this.api.sendMessage<any>(type, data);
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

