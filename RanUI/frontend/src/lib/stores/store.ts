import { get, writable } from 'svelte/store';
import { browser } from '$app/environment';
import type { TTP, ArmoryType, Node, Edge } from '$lib/model';
import * as runtime from "$lib/wailsjs/runtime";

interface Command {
	[key: string]: any
}

const addSubgraph = writable({});
const graph = writable({});
const removeSubgraph = writable({});
const alerts = writable("");

const RETRIES: number = 3;

const armory = writable<ArmoryType>({});
let socket: WebSocket | null = null;

const MAX_RETRIES = 3;
let retries: number = 0;

let hadError: boolean = false;


function onMessage(event) {
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
		debugger
		console.log(a);
	});
	runtime.EventsOn("armory-loaded", (data) => {
		debugger
		const msg = JSON.parse(data);
		let a = parseArmory(msg)
		armory.set(a);
		console.log(a);
	});
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

function parseArmory(data: ArmoryType | Object): ArmoryType {
	// this comes from the backend must be converted
	if (data && 'ttps' in data) {
		let armoryMap = new Map<string, TTP[]>();
		for (let [key, value] of Object.entries(data['ttps'] as Record<string, any>)) {
			let parsedValue = value as { [key: string]: any };
			let ttp: TTP = {
				id: parsedValue['ID'],
				name: parsedValue['Name'],
				description: parsedValue['Description'],
				// action: parsedValue['action'],
				tactics: [parsedValue['Tactic']],
				technique: parsedValue['Techniques'],
				requires: parsedValue['Requires'],
				params: parsedValue['Params']
			}
			armoryMap.set(key, value);
		}
		// Armory contains a CmdId field; process accordingly if needed.
		return armoryMap;
	}
	return data as ArmoryType;
}

const sendMessage = (msgType: string, command: Command) => {
	if (browser) {
		if (socket && socket.readyState == 1) {
			command.msg_type = msgType;
			socket.send(JSON.stringify(command));
		}
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
