export type TTP = {
    id: string;
    technique?: string;
    name: string;
    action: string;
    cmd_args?: object;
    tactics: string[];
    ms_id?: string;
    requires?: Object,
    params?: Object
};

export type Node = {
	id: string,
	name:string,
	parent?: string,
}

export type Edge = {
    source: string,
    target: string,
    label: string
}

export type ArmoryType = Map<string, TTP[]>;