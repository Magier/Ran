import { getContext, setContext } from "svelte";
import * as runtime from "$lib/wailsjs/runtime";
import type { ArmoryType, Node, Edge } from '$lib/model';
import { main, type domain } from '$lib/wailsjs/go/models';
import { GetGraph } from '$lib/wailsjs/go/main/App';

// Great video how to build stores in Svelte 5: https://www.youtube.com/watch?v=kMBDsyozllk


type Conditions = {
}

type Entity = {
    id: string;
    type?: string;
}


class CampaignState {
    targetSet: boolean = $state(false);
    activeConditions: Conditions = $state({});
    entities = $state<Entity[]>([]);
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
        console.log("connecting backend")
        // runtime.EventsOn("*", onMessage)
        runtime.EventsOn("*", (a) => {
            console.log(a);
        });
        runtime.EventsOn("armory-loaded", (data) => {
            this.armory = parseArmory(data)
        });
        runtime.EventsOn("facts-changed", (data) => {
            GetGraph().then((g: main.Graph) => {
                this.graph = g;
            });
        })

        GetGraph().then((g: main.Graph) => {
            this.graph = g;
        })
    }

    setInitialTarget(id: string) {
        this.targetSet = true;
        this.entities.push({ id: id })
     }

    isTargetSet() {
        return this.targetSet;
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