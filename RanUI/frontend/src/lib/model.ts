export enum AccessLevel {
    UserRead = 1,
    UserWrite = 2,
    UserExecute = 3,
    RootWriteRead = 4,
    RootWriteWrite = 5,
    RootWriteExecute = 6
}


export type TTP = {
    id: string;
    technique?: string;
    name: string;
    action: string;
    description: string;
    cmd_args?: object;
    tactic: string;
    ms_id?: string;
    requires?: Object,
    params?: Object
};

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

export type ArmoryType = Map<string, TTP[]>;