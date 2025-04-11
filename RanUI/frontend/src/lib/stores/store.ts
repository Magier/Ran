import { get, writable } from 'svelte/store';
import { browser } from '$app/environment';
import type { TTP, ArmoryType, Node, Edge } from '$lib/model';
import * as runtime from "$lib/wailsjs/runtime";
import { GetGraph } from '$lib/wailsjs/go/main/App';
import type { main } from '$lib/wailsjs/go/models';

interface Command {
	[key: string]: any
}

const addSubgraph = writable({});
const graph = writable({});
const removeSubgraph = writable({});
const alerts = writable("");

const RETRIES: number = 3;

const armory = writable<ArmoryType>(new Map());
let useWails: boolean = true;
let socket: WebSocket | null = null;

const MAX_RETRIES = 3;
let retries: number = 0;

let hadError: boolean = false;


function onMessage(event: { data: string; }) {
	console.group("Msg from WS")
	console.log(event.data);
	const msg = JSON.parse(event.data);
	const { type: msgType, data: data } = msg;
	console.log(`Type: ${msgType}`);
	console.log(data);
	console.groupEnd();

	switch (msgType) {
		case 'armory':
			armory.set(parseArmory(data));
			break;
		case 'topology':
			const [ns, es] = parse_topology(data);
			graph.set({ nodes: ns, edges: es });
		case 'addsubgraph':
			const [nodes, edges] = parse_topology(data);
			addSubgraph.set({ nodes: nodes, edges: edges });
			break;
		case 'removesubgraph':
			removeSubgraph.set({ nodes: data.entities, edges: data.relations });
			break;
		case 'error':
			alerts.set(data)
		default:
			console.log(`Received invalid message type ${msgType}: ${data}`);
			break;
	}
}

function handleDisconnect(event: CloseEvent) {
	// if it's error, then the socket was never ready and case is handled outside
	if (!hadError) {
		alerts.set("WebSocket connection lost ...")
	}
}

function connect(useSocket: boolean = false) {
	if (useSocket) {
		return connectSocket()
	} else {
		connectBackend()
	}
}

function connectSocket() {
	return new Promise(async (resolve, reject) => {
		// websocket is only available client side
		if (browser) {
			console.log("Prepping the socket in browser")
			socket = new WebSocket('ws://0.0.0.0:8080/ws');
			socket.addEventListener('open', function (event) {
				retries = 0;
				resolve(socket);
			});
			socket.onerror = (ev) => {
				hadError = true;
				reject(`Could not connect to websocket ${ev.target.url}`);
			}
			socket.onclose = handleDisconnect;
			socket.addEventListener('message', onMessage);
		}
	});
}


function connectBackend() {
	console.log("connecting backend")
	// runtime.EventsOn("*", onMessage)
	runtime.EventsOn("*", (a) => {
		console.log(a);
	});
	runtime.EventsOn("armory-loaded", (data) => {
		console.info("Armory loaded")
		let a = parseArmory(data)
		armory.set(a);
	});
	runtime.EventsOn("facts-changed", (data) => {
		// runtime.EventsOn("facts-changed", (data) => {
		// const facts = JSON.parse(data);
		// const [ns, es] = parseTopology(facts);
		GetGraph().then((g: main.Graph) => {
			console.log(g)
			graph.set(g);
		});
	})

	GetGraph().then((g: main.Graph) => {
		console.log(g)
		graph.set(g);
	})
}

function parse_topology(data: any): [Node[], Edge[]] {
	let nodes = [];
	for (let node of data.entities) {
		// define namespace as the nodes parent, if present
		if (node.ns !== undefined) {
			if (typeof node.ns === 'object') {
				node.parent = node.ns.id;
			} else if (typeof node.ns === 'string') {
				node.parent = node.ns;
			}
		}
		if (node.id === undefined) {
			node.id = node.name;
		}
		nodes.push(node);
	}

	let edges = [];
	for (let edge of data.relations) {
		edge.target = edge.destination;
		edge.id = `${edge.source}->${edge.target}`;
		edges.push(edge);
	}

	return [nodes, edges];
}

// function parseTopology(data: any): [Node[], Edge[]] {
// 	let nodes = [];
// 	for (let entity of data.NewEntities) {
// 		// define namespace as the nodes parent, if present
// 		if (entity.Namespace !== undefined) {
// 			if (typeof entity.Namespace === 'object') {
// 				entity.parent = entity.Namspace.Id;
// 			} else if (typeof entity.Namespace === 'string') {
// 				entity.parent = entity.Namespace;
// 			}
// 		}
// 		if (entity.Id === undefined) {
// 			entity.Id = entity.Name;
// 		}
// 		nodes.push(entity);
// 	}

// 	let edges = [];
// 	debugger
// 	for (let edge of data.NewRelations) {
// 		edge.target = edge.TargetId;
// 		edge.id = `${edge.SourceId}->${edge.target}`;
// 		edges.push(edge);
// 	}

// 	return [nodes, edges];
// }

function parseArmory(data: TTP[]): ArmoryType {
	// this comes from the backend must be converted
	let armoryMap = new Map<string, TTP[]>();
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

const sendMessage = (msgType: string, command: Command) => {
	if (useWails) {
		console.log(`Sending message ${msgType} to backend`)
		runtime.EventsEmit(msgType, JSON.stringify(command));
	}
	if (socket && socket.readyState == 1) {
		command.msg_type = msgType;
		socket.send(JSON.stringify(command));
	}
};

export default {
	connect: connect,
	// entities: entities.subscribe,
	addSubgraph: addSubgraph.subscribe,
	graph: graph.subscribe,
	removeSubgraph: removeSubgraph.subscribe,
	armory: armory.subscribe,
	onAlert: alerts.subscribe,
	connectBackend: connectBackend,
	sendMessage
};
