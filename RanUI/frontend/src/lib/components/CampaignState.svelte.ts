import { getContext, setContext } from "svelte";
import * as runtime from "$lib/wailsjs/runtime";
import type { ArmoryType, Node, Edge } from '$lib/model';
import { campaign, api, type domain } from '$lib/wailsjs/go/models';
import { GetArmory, GetCampaignState, GetGraph, GetRunningPods, ResetCampaign } from '$lib/wailsjs/go/main/App';
import { showToast, type ToastType } from '$lib/components/toaster';

// Great video how to build stores in Svelte 5: https://www.youtube.com/watch?v=kMBDsyozllk


type Conditions = {
}

export type Entity = {
    id: string;
    name: string;
    kind?: string;
    namespace?: string;
}

// type FactsDelta = {
//     Entities: Entity[];
//     Relations: domain.Relation[];
//     Identities: Identity[];
//     Assets: Asset[];
// }

// type FactsChanged = {
//     NewEntities:       Entity[]
//     NewRelations:      Relation[]
//     NewIdentities:     Identity[]
//     NewAssets:         Asset[]
//     RemovedEntities:   Entity[]
//     RemovedRelations:  Relation[]
//     RemovedIdentities: Identity[]
//     RemovedAssets:     Asset[]
// }

type ErrorMsg = {
    CmdId: string;
    Level: string;
    Msg: string;
}

type BackendError = {
    code: string;
    message: string;
}

class CampaignState {
    campaignId: number = $state(0);
    activeConditions: Conditions = $state({});
    entities = $state<Entity[]>([]);
    namespaces = $state<Entity[]>([]);
    pods = $state<Entity[]>([]);
    serviceAccounts = $state<Entity[]>([]);
    armory = $state<ArmoryType>(new Map());
    graph = $state<api.Graph>(new api.Graph());
    allPods: Entity[] = $state([]);

    connect(useSocket: boolean) {
        if (useSocket) {
            console.log("Connecting using socket is not yet supported in the new campaign context");
        } else {
            this.connectBackend();
        }
    }
    connectBackend() {
        console.info("CampaignState connecting to backend...");
        runtime.EventsOn("*", (a) => {
            console.log(a);
        });
        runtime.EventsOn("armory-loaded", (data) => {
            this.armory = parseArmory(data)
        });
        runtime.EventsOn("facts-changed", (dataStr: string) => {
            GetGraph().then((g: api.Graph) => { this.graph = g; });
            // TODO: properly update state based on the received fact changes
            const data = JSON.parse(dataStr);
            GetCampaignState().then((s: api.CampaignState) => { this.#setState(s); })
        })
        runtime.EventsOn("error-msg", (rawMsg: string) => {
            let msg: ErrorMsg = JSON.parse(rawMsg);

            // Map msg.Level to ToastType
            let toastType: ToastType;
            switch (msg.Level) {
                case "ERROR":
                case "WARN":
                case "FATAL":
                    toastType = "error";
                    break;
                case "INFO":
                case "DEBUG":
                default:
                    toastType = "info";
            }

            showToast("Error", msg.Msg, toastType);
        });

        function showError(msg: string | object) {
            if (typeof msg === 'object') {
                if (msg.hasOwnProperty('code') && (msg as BackendError).code == "GO_BOUND_METHOD_ERROR") {
                    msg = (msg as any).message;
                } else { // fallback handling to show full object (may allow later refinement)
                    msg = JSON.stringify(msg);
                }
            } else if (typeof msg !== 'string') {
                msg = String(msg);
            }

            console.error(msg);
            showToast("Error", JSON.stringify(msg), "error");
        }

        GetGraph().then((g: api.Graph) => { this.graph =g; })
        GetCampaignState().then((s: api.CampaignState) => { this.#setState(s); })
        GetArmory().then((a: domain.TTP[]) => { this.armory = parseArmory(a); })
        GetRunningPods("").then(pods => {this.allPods = pods;}).catch(showError);
    }

    reset() {
        this.entities = [];
        this.namespaces = [];
        this.pods = [];
        this.serviceAccounts = [];
        this.campaignId += 1; // Increment campaign ID, to trigger changes based on new campaign
        ResetCampaign().then(() => {
            GetGraph().then((g: api.Graph) => { this.graph = g; });
        });
    }

    #setState(state: api.CampaignState): void {
        let entities = [];
        for (const [id, entity] of Object.entries(state.entities || {})) {
            if (entity.kind === 'Namespace') {
                this.namespaces = [...this.namespaces, entity];
            } else if (entity.kind === 'Pod') {
                this.pods = [...this.pods, entity];
            } else if (entity.kind === 'ServiceAccount') {
                this.serviceAccounts = [...this.serviceAccounts, entity];
            }

            if (!entity.id) {
                entity.id = id;
            }
            entities.push(entity);
        }
        this .entities = entities; // ensure we replace the array to trigger reactivity
    }

    #updateState(state: api.CampaignState): void {
        for (const [id, entity] of Object.entries(state.entities || [])) {
            if (!this.entities.some(e => e.id === entity.id)) {
                this.entities = [...this.entities, entity];
            } else {
                // TODO: properly update the entity
            }
            if (entity.kind === 'Namespace') {
                this.namespaces = [...this.namespaces, entity];
            }
        }

        this.entities = state.entities.map((node: Node) => {
            debugger
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

    getTtpById(id: string): domain.TTP | undefined {
        for (const [group, ttps] of this.armory) {
            const ttp = ttps.find(t => t.id === id);
            if (ttp) {
                return ttp;
            }
        }
    }

    getNamespaces(): Entity[] {
        let ns = this.entities.filter(entity => entity.kind === 'Namespace')
        return ns || [];
    }

    getPods(ns?: string, all: boolean = false): Entity[] {
        // go beyond regular campaign state and return all pods in the cluster (regardless of exploration)
        if (all) { 
            return this.allPods;
        }
        let pods = this.entities.filter(entity => entity.kind === 'Pod' && (!ns || entity.namespace === ns));
        return pods || [];
    }

    getServiceAccounts(ns?: string, permissions?: string[], includeUnkwnon?: boolean): Entity[] {
        let serviceAccounts = this.entities.filter(entity => entity.kind === 'ServiceAccount');
        if (ns) {
            serviceAccounts = serviceAccounts.filter(entity => entity.namespace === ns);
        }
        return serviceAccounts || [];
    }
}

const DEFAULT_KEY = '$_campaignState';

export const getCampaignState = (key = DEFAULT_KEY) => {
    return getContext<CampaignState>(key);
}

export const setCampaignState = (key = DEFAULT_KEY) => {
    const campaignState = new CampaignState();
    return setContext(key, campaignState);
}



export function parseArmory(data: domain.TTP[]): ArmoryType {
    // this comes from the backend must be converted
    let armoryMap = new Map<string, domain.TTP[]>();
    for (let ttp of data) {
        let groupName = ttp.tactic;
        if (groupName === "") {
            groupName = "Other";
        }
        if (!armoryMap.has(groupName)) {
            armoryMap.set(groupName, []);
        }
        armoryMap.get(groupName)!.push(ttp);
    }
    // Armory contains a CmdId field; process accordingly if needed.
    return armoryMap;
}