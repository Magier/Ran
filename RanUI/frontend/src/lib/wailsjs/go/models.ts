export namespace main {
	
	export class Edge {
	    id: string;
	    label: string;
	    sourceId: string;
	    targetId: string;
	
	    static createFrom(source: any = {}) {
	        return new Edge(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.id = source["id"];
	        this.label = source["label"];
	        this.sourceId = source["sourceId"];
	        this.targetId = source["targetId"];
	    }
	}
	export class Node {
	    id: string;
	    name: string;
	    kind: string;
	    parent: string;
	    ip: string;
	    username: string;
	    accessLevel: string;
	    os: string;
	    version: string;
	
	    static createFrom(source: any = {}) {
	        return new Node(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.id = source["id"];
	        this.name = source["name"];
	        this.kind = source["kind"];
	        this.parent = source["parent"];
	        this.ip = source["ip"];
	        this.username = source["username"];
	        this.accessLevel = source["accessLevel"];
	        this.os = source["os"];
	        this.version = source["version"];
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

