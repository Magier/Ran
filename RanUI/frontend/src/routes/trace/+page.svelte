<script lang="ts">
	import { GetTrace } from '$lib/wailsjs/go/main/App';
	import { main } from '$lib/wailsjs/go/models';
	import AttackStepDetails from '$lib/components/attack_step_details.svelte';
	import {
		SvelteFlow,
		Position,
		Controls,
		Background,
		BackgroundVariant,
		MiniMap,
		type Node,
		type Edge
	} from '@xyflow/svelte';
	import dagre from '@dagrejs/dagre';
	import { writable } from 'svelte/store';
	import '@xyflow/svelte/dist/style.css';
	import { Modal } from '@skeletonlabs/skeleton-svelte';

	const dagreGraph = new dagre.graphlib.Graph();
	dagreGraph.setDefaultEdgeLabel(() => ({}));

	let nodes = writable<Node[]>([]);
	let edges = writable<Edge[]>([]);

	// const snapGrid = [25, 25];

	const nodeWidth = 172;
	const nodeHeight = 36;
	let selectedStep: campaign.AttackStep | null = $state(null);

	function convertStep(step: campaign.AttackStep): Node {
		return {
			id: step.ID,
			type: 'default',
			data: { label: step.TTP.name, step: step },
			position: { x: 0, y: 0 } // will be replaced by the layout algorithm
		};
	}

	function convertEdge(edge: main.Edge): Edge {
		return {
			id: edge.id,
			type: 'default',
			source: edge.sourceId,
			target: edge.targetId,
			label: edge.name
		};
	}

	function layOutElements(nodes: Node[], edges: Edge[], direction = 'TB') {
		const isHorizontal = direction === 'LR';
		dagreGraph.setGraph({ rankdir: direction });

		nodes.forEach((node) => {
			dagreGraph.setNode(node.id, { width: nodeWidth, height: nodeHeight });
		});

		edges.forEach((edge) => {
			dagreGraph.setEdge(edge.source, edge.target);
		});

		dagre.layout(dagreGraph);

		nodes.forEach((node) => {
			// set the position of the input/output ports
			const nodeWithPosition = dagreGraph.node(node.id);
			node.targetPosition = isHorizontal ? Position.Left : Position.Top;
			node.sourcePosition = isHorizontal ? Position.Right : Position.Bottom;

			// We are shifting the dagre node position (anchor=center center) to the top left
			// so it matches the React Flow node anchor point (top left).
			node.position = {
				x: nodeWithPosition.x - nodeWidth / 2,
				y: nodeWithPosition.y - nodeHeight / 2
			};
		});
		return { nodes, edges };
	}

	GetTrace()
		.then((result: main.AttackFlow) => {
			console.log('Graph:', result);
			const { steps, edges: es } = result;
			const laidOutElements = layOutElements(steps.map(convertStep), es.map(convertEdge), 'TB');

			nodes.set(laidOutElements.nodes);
			edges.set(laidOutElements.edges);
		})
		.catch((err) => {
			console.error(err);
		});
</script>

<div class="items-top mx-auto flex h-dvh w-full justify-center">
	<!-- {#each nodes as node}
		<div class="border-primary-950-50 m-4 rounded-lg border-2 p-6 shadow-lg">
			{node.name}
		</div>
	{/each} -->
	<!-- <Graph bind:selectedNodeId /> -->

	<SvelteFlow
		id="testing"
		{nodes}
		{edges}
		fitView
		colorMode="dark"
		on:nodeclick={(event) => {
			selectedStep = event.detail.node.data.step as campaign.AttackStep;
		}}
		on:paneclick={(event) => {
			selectedStep = null;
		}}
		on:selectionclick={(event) => console.log('on selection click', event, event.detail)}
		minZoom={0.1}
		maxZoom={2.5}
	>
		<Controls />
		<Background variant={BackgroundVariant.Dots} />
		<!-- <MiniMap /> -->
	</SvelteFlow>
</div>

<Modal
	open={selectedStep !== null}
	onOpenChange={(e) => {
		console.log('Modal open:', e.open);
		if (!e.open) {
			selectedStep = null;
		}
	}}
	contentBase="bg-surface-100-900 p-4 space-y-4 shadow-xl w-[480px] h-screen"
	positionerJustify="justify-end"
	positionerAlign=""
	positionerPadding=""
	transitionsPositionerIn={{
		x: 480,
		duration: 200
	}}
	transitionsPositionerOut={{ x: 480, duration: 200 }}
>
	<!-- {#snippet trigger()}Open Drawer{/snippet} -->
	{#snippet content()}
		<AttackStepDetails step={selectedStep!} />
	{/snippet}
</Modal>
