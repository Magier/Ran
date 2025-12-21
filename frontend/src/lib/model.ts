import type { domain } from "./domain/models";

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
    // example id is "ns/<namespace>/<kind>/<name>"
    // Check if the string starts with "ns/"
    if (!entityId.startsWith('ns/')) {
        throw new Error('Entity ID must start with "ns/"');
    }

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
        throw new Error('Invalid entity ID format');
    }

    // Extract the components
    const ns = parts[1];
    const kind = parts[2];
    const name = parts[3];

    return { name, namespace: ns, kind };
}



export type ArmoryType = Map<string, domain.TTP[]>;