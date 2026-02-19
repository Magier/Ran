import type { TTP } from "$lib/api/index";

export enum AccessLevel {
    UserRead = 1,
    UserWrite = 2,
    UserExecute = 3,
    RootWriteRead = 4,
    RootWriteWrite = 5,
    RootWriteExecute = 6
}


export type Param = {
    Name: string;
    Type: string;
    Default: string;
    Description: string;
    Required: boolean;
}


export type Node = {
    id: string,
    name: string,
    parent?: string,
}

export type Edge = {
    source: string,
    target: string,
    label: string
}



export type EntityId = {
    name: string
    namespace: string
    kind: string
}

export function parseEntityId(entityId: string): EntityId {
    // Namespaced resource: "ns/<namespace>/<kind>/<name>"
    // Cluster-wide resource: "<kind>/<name>"
    
    // Check if the string starts with "ns/" (namespaced resource)
    if (entityId.startsWith('ns/')) {
        // Split the string by '/'
        const parts = entityId.split('/');

        // We expect at least 4 parts: ["ns", "<namespace>", "<kind>", "<name>"]
        if (parts.length == 2) {
            return {
                name: parts[1],
                namespace: '',
                kind: 'namespace'
            }
        }
        if (parts.length < 4) {
            throw new Error('Invalid namespaced entity ID format');
        }

        // Extract the components
        const ns = parts[1];
        const kind = parts[2];
        const name = parts[3];

        return { name, namespace: ns, kind };
    } else {
        // Cluster-wide resource: <kind>/<name>
        const parts = entityId.split('/');
        
        if (parts.length < 2) {
            throw new Error('Invalid cluster-wide entity ID format');
        }
        
        const kind = parts[0];
        const name = parts[1];
        
        return { name, namespace: '', kind };
    }
}



export type ArmoryType = Map<string, TTP[]>;