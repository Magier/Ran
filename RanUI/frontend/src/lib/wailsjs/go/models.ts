export namespace campaign {
	
	export class AttackStep {
	    ID: string;
	    TTP: domain.TTP;
	    Success: boolean;
	    Command: string;
	    Results: string[];
	    // Go type: time
	    StartAt: any;
	    Target: any;
	    // Go type: time
	    CompletedAt: any;
	    Observables: any[];
	
	    static createFrom(source: any = {}) {
	        return new AttackStep(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.ID = source["ID"];
	        this.TTP = this.convertValues(source["TTP"], domain.TTP);
	        this.Success = source["Success"];
	        this.Command = source["Command"];
	        this.Results = source["Results"];
	        this.StartAt = this.convertValues(source["StartAt"], null);
	        this.Target = source["Target"];
	        this.CompletedAt = this.convertValues(source["CompletedAt"], null);
	        this.Observables = source["Observables"];
	    }
	
		convertValues(a: any, classs: any, asMap: boolean = false): any {
		    if (!a) {
		        return a;
		    }
		    if (a.slice && a.map) {
		        return (a as any[]).map(elem => this.convertValues(elem, classs));
		    } else if ("object" === typeof a) {
		        if (asMap) {
		            for (const key of Object.keys(a)) {
		                a[key] = new classs(a[key]);
		            }
		            return a;
		        }
		        return new classs(a);
		    }
		    return a;
		}
	}

}

export namespace domain {
	
	export class CodeSnippet {
	    Lang: string;
	    Code: string;
	    Parameters: Record<string, string>;
	    EnvVars: string[];
	
	    static createFrom(source: any = {}) {
	        return new CodeSnippet(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.Lang = source["Lang"];
	        this.Code = source["Code"];
	        this.Parameters = source["Parameters"];
	        this.EnvVars = source["EnvVars"];
	    }
	}
	export class CmdVariant {
	    Key: string;
	    Command: string;
	    IsLocalCommand: boolean;
	    Execute: CodeSnippet;
	    Cleanup: CodeSnippet;
	
	    static createFrom(source: any = {}) {
	        return new CmdVariant(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.Key = source["Key"];
	        this.Command = source["Command"];
	        this.IsLocalCommand = source["IsLocalCommand"];
	        this.Execute = this.convertValues(source["Execute"], CodeSnippet);
	        this.Cleanup = this.convertValues(source["Cleanup"], CodeSnippet);
	    }
	
		convertValues(a: any, classs: any, asMap: boolean = false): any {
		    if (!a) {
		        return a;
		    }
		    if (a.slice && a.map) {
		        return (a as any[]).map(elem => this.convertValues(elem, classs));
		    } else if ("object" === typeof a) {
		        if (asMap) {
		            for (const key of Object.keys(a)) {
		                a[key] = new classs(a[key]);
		            }
		            return a;
		        }
		        return new classs(a);
		    }
		    return a;
		}
	}
	
	export class HttpCmd {
	    Endpoint: string;
	    Method: string;
	    Args: string[];
	    Headers: Record<string, string>;
	    Body: string;
	
	    static createFrom(source: any = {}) {
	        return new HttpCmd(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.Endpoint = source["Endpoint"];
	        this.Method = source["Method"];
	        this.Args = source["Args"];
	        this.Headers = source["Headers"];
	        this.Body = source["Body"];
	    }
	}
	export class Parameter {
	    Name: string;
	    Type: string;
	    Required: boolean;
	    Description: string;
	    Examples: string[];
	    Default: string;
	
	    static createFrom(source: any = {}) {
	        return new Parameter(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.Name = source["Name"];
	        this.Type = source["Type"];
	        this.Required = source["Required"];
	        this.Description = source["Description"];
	        this.Examples = source["Examples"];
	        this.Default = source["Default"];
	    }
	}
	export class AccessLevel {
	
	
	    static createFrom(source: any = {}) {
	        return new AccessLevel(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	
	    }
	}
	export class Requirements {
	    Kind: string;
	    // Go type: AccessLevel
	    accessLevel: any;
	    RbacPermission: string;
	    State: Record<string, number>;
	    Exists: string[];
	    OtherFields: Record<string, any>;
	
	    static createFrom(source: any = {}) {
	        return new Requirements(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.Kind = source["Kind"];
	        this.accessLevel = this.convertValues(source["accessLevel"], null);
	        this.RbacPermission = source["RbacPermission"];
	        this.State = source["State"];
	        this.Exists = source["Exists"];
	        this.OtherFields = source["OtherFields"];
	    }
	
		convertValues(a: any, classs: any, asMap: boolean = false): any {
		    if (!a) {
		        return a;
		    }
		    if (a.slice && a.map) {
		        return (a as any[]).map(elem => this.convertValues(elem, classs));
		    } else if ("object" === typeof a) {
		        if (asMap) {
		            for (const key of Object.keys(a)) {
		                a[key] = new classs(a[key]);
		            }
		            return a;
		        }
		        return new classs(a);
		    }
		    return a;
		}
	}
	export class TTP {
	    id: string;
	    name: string;
	    description: string;
	    tactic: string;
	    techniques: string[];
	    references: string[];
	    cmdVariants: CmdVariant[];
	    httpCmd: HttpCmd;
	    params: Parameter[];
	    CommandMsg: any;
	    requires: Requirements;
	    effects: string[];
	    Parser: string;
	
	    static createFrom(source: any = {}) {
	        return new TTP(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.id = source["id"];
	        this.name = source["name"];
	        this.description = source["description"];
	        this.tactic = source["tactic"];
	        this.techniques = source["techniques"];
	        this.references = source["references"];
	        this.cmdVariants = this.convertValues(source["cmdVariants"], CmdVariant);
	        this.httpCmd = this.convertValues(source["httpCmd"], HttpCmd);
	        this.params = this.convertValues(source["params"], Parameter);
	        this.CommandMsg = source["CommandMsg"];
	        this.requires = this.convertValues(source["requires"], Requirements);
	        this.effects = source["effects"];
	        this.Parser = source["Parser"];
	    }
	
		convertValues(a: any, classs: any, asMap: boolean = false): any {
		    if (!a) {
		        return a;
		    }
		    if (a.slice && a.map) {
		        return (a as any[]).map(elem => this.convertValues(elem, classs));
		    } else if ("object" === typeof a) {
		        if (asMap) {
		            for (const key of Object.keys(a)) {
		                a[key] = new classs(a[key]);
		            }
		            return a;
		        }
		        return new classs(a);
		    }
		    return a;
		}
	}

}

export namespace main {
	
	export enum AccessLevel {
	    NoAccess = "NoAccess",
	    UserRead = "UserRead",
	    UserExec = "UserExec",
	    RootRead = "RootRead",
	    RootExec = "RootExec",
	}
	export class Edge {
	    id: string;
	    name: string;
	    sourceId: string;
	    targetId: string;
	
	    static createFrom(source: any = {}) {
	        return new Edge(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.id = source["id"];
	        this.name = source["name"];
	        this.sourceId = source["sourceId"];
	        this.targetId = source["targetId"];
	    }
	}
	export class AttackFlow {
	    steps: campaign.AttackStep[];
	    edges: Edge[];
	    rootNodeId: string;
	
	    static createFrom(source: any = {}) {
	        return new AttackFlow(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.steps = this.convertValues(source["steps"], campaign.AttackStep);
	        this.edges = this.convertValues(source["edges"], Edge);
	        this.rootNodeId = source["rootNodeId"];
	    }
	
		convertValues(a: any, classs: any, asMap: boolean = false): any {
		    if (!a) {
		        return a;
		    }
		    if (a.slice && a.map) {
		        return (a as any[]).map(elem => this.convertValues(elem, classs));
		    } else if ("object" === typeof a) {
		        if (asMap) {
		            for (const key of Object.keys(a)) {
		                a[key] = new classs(a[key]);
		            }
		            return a;
		        }
		        return new classs(a);
		    }
		    return a;
		}
	}
	
	export class Entitlement {
	    verbs: string[];
	    resourceTypes: string[];
	    resourceNames: string[];
	    namespace: string;
	
	    static createFrom(source: any = {}) {
	        return new Entitlement(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.verbs = source["verbs"];
	        this.resourceTypes = source["resourceTypes"];
	        this.resourceNames = source["resourceNames"];
	        this.namespace = source["namespace"];
	    }
	}
	export class Node {
	    id: string;
	    name: string;
	    kind: string;
	    parent: string;
	    accessLevel: string;
	    entitlements: Entitlement[];
	    entity: any;
	
	    static createFrom(source: any = {}) {
	        return new Node(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.id = source["id"];
	        this.name = source["name"];
	        this.kind = source["kind"];
	        this.parent = source["parent"];
	        this.accessLevel = source["accessLevel"];
	        this.entitlements = this.convertValues(source["entitlements"], Entitlement);
	        this.entity = source["entity"];
	    }
	
		convertValues(a: any, classs: any, asMap: boolean = false): any {
		    if (!a) {
		        return a;
		    }
		    if (a.slice && a.map) {
		        return (a as any[]).map(elem => this.convertValues(elem, classs));
		    } else if ("object" === typeof a) {
		        if (asMap) {
		            for (const key of Object.keys(a)) {
		                a[key] = new classs(a[key]);
		            }
		            return a;
		        }
		        return new classs(a);
		    }
		    return a;
		}
	}
	export class Graph {
	    nodes: Node[];
	    edges: Edge[];
	    rootNodeId: string;
	
	    static createFrom(source: any = {}) {
	        return new Graph(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.nodes = this.convertValues(source["nodes"], Node);
	        this.edges = this.convertValues(source["edges"], Edge);
	        this.rootNodeId = source["rootNodeId"];
	    }
	
		convertValues(a: any, classs: any, asMap: boolean = false): any {
		    if (!a) {
		        return a;
		    }
		    if (a.slice && a.map) {
		        return (a as any[]).map(elem => this.convertValues(elem, classs));
		    } else if ("object" === typeof a) {
		        if (asMap) {
		            for (const key of Object.keys(a)) {
		                a[key] = new classs(a[key]);
		            }
		            return a;
		        }
		        return new classs(a);
		    }
		    return a;
		}
	}

}

