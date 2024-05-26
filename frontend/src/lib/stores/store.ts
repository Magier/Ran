import { get, writable } from 'svelte/store';
import { browser } from '$app/environment';
import type { TTP, ArmoryType, Node, Edge } from '../../routes/model';

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

const MAX_RETRIES= 3;
let retries: number = 0;

let hadError: boolean = false;


function onMessage(event){
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


let isReady = new Promise(async (resolve, reject) => {
// websocket is only available client side
	if (browser) {
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

function parseArmory(armory: ArmoryType) {
	return armory
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
	isReady: isReady,
	// entities: entities.subscribe,
	addSubgraph: addSubgraph.subscribe,
	graph: graph.subscribe,
	removeSubgraph: removeSubgraph.subscribe,
	armory: armory.subscribe,
	onAlert: alerts.subscribe,
	sendMessage
};
