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
};

export type Relation = {
	id: string;
	source: string;
	destination: string;
	kind: string;
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
	namespaces = $state<Entity[]>([]);
	pods = $state<Entity[]>([]);
	serviceAccounts = $state<Entity[]>([]);
	armory = $state<ArmoryType>(new Map());
	graph = $state<Graph>({} as Graph);
	allPods: Entity[] = $state([]);
	pendingMessages: string[] = [];
	api: RanAPI = $state(getRanAPI());

	init(url?: string): Promise<void> {
		// this.api.onmessage = this.handleMessage;
		// If no URL provided, it will auto-construct from window.location

		this.api.on('armory-loaded', (data) => {
			this.armory = parseArmory(data);
		});
		this.api.on('facts-changed', (data: any) => {
			this.api.GetGraph().then((g: Graph) => {
				this.graph = g;
			});
			// TODO: properly update state based on the received fact changes
			this.api.GetCampaignState().then((s: State) => {
				this.#setState(s);
			});
		});
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
					console.info('All pods:', pods);
				})
				.catch(this.showError);
		});
	}

	handleMessage(event: MessageEvent) {
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

	#setState(state: State): void {
		let entities = [];
		for (const [id, entity] of Object.entries(state.entities || {})) {
			const typedEntity = entity as Entity;
			if (typedEntity.kind === 'Namespace') {
				this.namespaces = [...this.namespaces, typedEntity];
			} else if (typedEntity.kind === 'Pod') {
				this.pods = [...this.pods, typedEntity];
			} else if (typedEntity.kind === 'ServiceAccount') {
				this.serviceAccounts = [...this.serviceAccounts, typedEntity];
			}

			if (!typedEntity.id) {
				typedEntity.id = id;
			}
			entities.push(typedEntity);
		}
		this.entities = entities; // ensure we replace the array to trigger reactivity
	}

	#updateState(state: State): void {
		for (const [id, entity] of Object.entries(state.entities || [])) {
			if (!this.entities.some((e) => e.id === entity.id)) {
				this.entities = [...this.entities, entity];
			} else {
				// TODO: properly update the entity
			}
			if (entity.kind === 'Namespace') {
				this.namespaces = [...this.namespaces, entity];
			}
		}

		this.entities = state.entities.map((node: Node) => {
			return {
				id: node.id,
				name: node.name,
				kind: node.kind,
				namespace: node.namespace
			};
		});
	}

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
		if (id ===  "") {
			return undefined;
		}
		return this.entities.find((entity) => entity.id === id);
	}

	getRelationById(id: string): Relation | undefined {
		if (id ===  "") {
			return undefined;
		}
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



	getServiceAccounts(ns?: string, permissions?: string[], includeUnkwnon?: boolean): Entity[] {
		let serviceAccounts = this.entities.filter((entity) => entity.kind === 'ServiceAccount');
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

