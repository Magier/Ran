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
	    ExecutedOn: any;
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
	        this.ExecutedOn = source["ExecutedOn"];
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
	
	export class AccessLevel {
	    User: number;
	    Level: number;
	
	    static createFrom(source: any = {}) {
	        return new AccessLevel(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.User = source["User"];
	        this.Level = source["Level"];
	    }
	}
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
	export class FactsChanged {
	    CmdId: string;
	    NewEntities: any[];
	    NewRelations: any[];
	    NewIdentities: any[];
	    NewAssets: any[];
	    RemovedEntities: any[];
	    RemovedRelations: any[];
	    RemovedIdentities: any[];
	    RemovedAssets: any[];
	
	    static createFrom(source: any = {}) {
	        return new FactsChanged(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.CmdId = source["CmdId"];
	        this.NewEntities = source["NewEntities"];
	        this.NewRelations = source["NewRelations"];
	        this.NewIdentities = source["NewIdentities"];
	        this.NewAssets = source["NewAssets"];
	        this.RemovedEntities = source["RemovedEntities"];
	        this.RemovedRelations = source["RemovedRelations"];
	        this.RemovedIdentities = source["RemovedIdentities"];
	        this.RemovedAssets = source["RemovedAssets"];
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
	export class Procedure {
	    Key: string;
	    Command: string;
	    Tool: string;
	    IsLocalCommand: boolean;
	    Execute: CodeSnippet;
	    Cleanup: CodeSnippet;
	
	    static createFrom(source: any = {}) {
	        return new Procedure(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.Key = source["Key"];
	        this.Command = source["Command"];
	        this.Tool = source["Tool"];
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
	export class RBACPermission {
	    verb: string;
	    resourceName: string;
	    resourceType: string;
	    apiGroup: string;
	    scope: string;
	    sourceRole: string;
	
	    static createFrom(source: any = {}) {
	        return new RBACPermission(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.verb = source["verb"];
	        this.resourceName = source["resourceName"];
	        this.resourceType = source["resourceType"];
	        this.apiGroup = source["apiGroup"];
	        this.scope = source["scope"];
	        this.sourceRole = source["sourceRole"];
	    }
	}
	export class Requirements {
	    Kind: string;
	    accessLevel: AccessLevel;
	    rbac: RBACPermission;
	    Exists: string[];
	    OtherFields: Record<string, any>;
	
	    static createFrom(source: any = {}) {
	        return new Requirements(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.Kind = source["Kind"];
	        this.accessLevel = this.convertValues(source["accessLevel"], AccessLevel);
	        this.rbac = this.convertValues(source["rbac"], RBACPermission);
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
	export class State {
	    entitlements?: Record<string, string[]>;
	    entityCounts?: Record<string, number>;
	
	    static createFrom(source: any = {}) {
	        return new State(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.entitlements = source["entitlements"];
	        this.entityCounts = source["entityCounts"];
	    }
	}
	export class TTP {
	    id: string;
	    name: string;
	    description: string;
	    tactic: string;
	    techniques: string[];
	    status: string;
	    references: string[];
	    procedures: Procedure[];
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
	        this.status = source["status"];
	        this.references = source["references"];
	        this.procedures = this.convertValues(source["procedures"], Procedure);
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
	
	export class Edge {
	    id: string;
	    name: string;
	    weight: number;
	    relation: any;
	    sourceId: string;
	    targetId: string;
	
	    static createFrom(source: any = {}) {
	        return new Edge(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.id = source["id"];
	        this.name = source["name"];
	        this.weight = source["weight"];
	        this.relation = source["relation"];
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
	
	export class Node {
	    id: string;
	    name: string;
	    kind: string;
	    parent: string;
	    accessLevel: string;
	    entity: any;
	    compromised: boolean;
	
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
	        this.entity = source["entity"];
	        this.compromised = source["compromised"];
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
	export class K8sResource {
	    id: string;
	    name: string;
	    namespace: string;
	    kind: string;
	
	    static createFrom(source: any = {}) {
	        return new K8sResource(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.id = source["id"];
	        this.name = source["name"];
	        this.namespace = source["namespace"];
	        this.kind = source["kind"];
	    }
	}

}

