import { getContext, setContext } from "svelte";
import * as runtime from "$lib/wailsjs/runtime";
import type { ArmoryType, Node, Edge, Relation } from '$lib/model';
import { campaign, main, type domain } from '$lib/wailsjs/go/models';
import { GetCampaignState, GetGraph, ResetCampaign } from '$lib/wailsjs/go/main/App';
import { showToast, type ToastType } from '$lib/components/toaster';

// Great video how to build stores in Svelte 5: https://www.youtube.com/watch?v=kMBDsyozllk


type Conditions = {
}

type Entity = {
    id: string;
    name: string;
    kind?: string;
    namespace?: string;
}

type FactsDelta = {
    Entities: Entity[];
    Relations: Relation[];
    Identities: Identity[];
    Assets: Asset[];
}

type FactsChanged = {
    NewEntities:       Entity[]
    NewRelations:      Relation[]
    NewIdentities:     Identity[]
    NewAssets:         Asset[]
    RemovedEntities:   Entity[]
    RemovedRelations:  Relation[]
    RemovedIdentities: Identity[]
    RemovedAssets:     Asset[]
}

type ErrorMsg = {
    CmdId: string;
    Level: string;
    Msg: string;
}

class CampaignState {
    campaignId: number = $state(0);
    activeConditions: Conditions = $state({});
    entities = $state<Entity[]>([]);
    namespaces = $state<Entity[]>([]);
    pods = $state<Entity[]>([]);
    serviceAccounts = $state<Entity[]>([]);
    armory = $state<ArmoryType>(new Map());
    graph = $state<main.Graph>(new main.Graph());

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
            GetGraph().then((g: main.Graph) => { this.graph = g; });
            // TODO: properly update state based on the received fact changes
            const data = JSON.parse(dataStr);
            GetCampaignState().then((s: main.CampaignState) => { this.#setState(s); })
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

        GetGraph().then((g: main.Graph) => { this.graph =g; })
        GetCampaignState().then((s: main.CampaignState) => { this.#setState(s); })
    }

    reset() {
        this.entities = [];
        this.namespaces = [];
        this.pods = [];
        this.serviceAccounts = [];
        this.campaignId += 1; // Increment campaign ID, to trigger changes based on new campaign
        ResetCampaign().then(() => {
            GetGraph().then((g: main.Graph) => { this.graph = g; });
        });
    }

    #setState(state: main.CampaignState): void {
        this.entities = [];

        for (const entity of state.entities || []) {
            if (entity.kind === 'Namespace') {
                this.namespaces = [...this.namespaces, entity];
            } else if (entity.kind === 'Pod') {
                this.pods = [...this.pods, entity];
            } else if (entity.kind === 'ServiceAccount') {
                this.serviceAccounts = [...this.serviceAccounts, entity];
            }

            this.entities.push(entity.entity);
        }

        this.entities = state.entities.map((entity: Entity) => {
            return {
                id: entity.id,
                name: entity.name,
                kind: entity.kind,
                namespace: entity.namespace
            };
        });
    }

    #updateState(state: main.CampaignState): void {
        for (const entity of state.entities || []) {
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
            return {
                id: node.id,
                name: node.name,
                kind: node.kind,
                namespace: node.entity?.namespace
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

    getPods(ns?: string): Entity[] {
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