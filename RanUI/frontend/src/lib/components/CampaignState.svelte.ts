import { getContext, setContext } from "svelte";
import * as runtime from "$lib/wailsjs/runtime";
import type { ArmoryType, Node, Edge, Relation } from '$lib/model';
import { main, type domain } from '$lib/wailsjs/go/models';
import { GetGraph } from '$lib/wailsjs/go/main/App';

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

class CampaignState {
    activeConditions: Conditions = $state({});
    entities = $state<Entity[]>([]);
    namespaces = $state<string[]>([]);
    pods = $state<string[]>([]);
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
        console.warn("Connecting to backend...");
        runtime.EventsOn("*", (a) => {
            console.log(a);
        });
        runtime.EventsOn("armory-loaded", (data) => {
            this.armory = parseArmory(data)
        });
        runtime.EventsOn("facts-changed", (dataStr: string) => {
            const data = JSON.parse(dataStr);
            this.entities = this.updateEntities(data);
            console.log("Facts changed: ", data);
            console.log("Campaign now has " + this.entities.length + " entities");
            GetGraph().then((g: main.Graph) => { this.graph = g; });
        })

        GetGraph().then((g: main.Graph) => { this.graph = g; })
    }

    updateEntities(data: FactsChanged): Entity[] {
        // Update the entities based on the facts changed
        for (const entity of data.RemovedEntities || []) {
            this.entities = this.entities.filter(e => e.id !== entity.id);
        }

        for (const entity of data.NewEntities || []) {
            if (!this.entities.some(e => e.id === entity.id)) {
                console.log("Adding entity: ", entity);
                this.entities = [...this.entities, entity];
            } else {
                // TODO: properly update the entity
            }
            if (entity.kind === 'Namespace') {
                this.namespaces = [...this.namespaces, entity.id];
            }
        }
        return this.entities
    }

    getTtpById(id: string): domain.TTP | undefined {
        for (const [group, ttps] of this.armory) {
            const ttp = ttps.find(t => t.id === id);
            if (ttp) {
                return ttp;
            }
        }
    }

    getNamespaces(): string[] {
        let ns = this.entities.filter(entity => entity.kind === 'Namespace').map(entity => entity.id);
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