<script lang="ts">
	import { onMount } from 'svelte';
	import cytoscape from 'cytoscape';
	import dagre from 'cytoscape-dagre';

	import { getGraphStyle, layout } from './graph_style';
	import store from '$lib/stores/store.js';
	import type { main } from '$lib/wailsjs/go/models';

	type GraphProps = {
		class?: string;
		selectedNodeId: string;
		selectedNode?: main.Node;
	};
	let {
		class: className = '',
		selectedNodeId = $bindable(),
		selectedNode = $bindable()
	}: GraphProps = $props();

	let nodes = $state([]);
	let edges = $state([]);

	let graphContainer = $state();
	let cy: cytoscape.Core;

	cytoscape.use(dagre);

	function handleSelection(event: cytoscape.Event) {
		let el = event.target;
		selectedNode = el.data();
		selectedNodeId = el.data()['id'];
		console.log(el.data());
		console.log(el.classes());
	}

	function resetSelection(event: cytoscape.Event) {
		selectedNode = undefined;
		selectedNodeId = '';
	}

	type CyNode = {
		id: string;
		label: string;
		data: main.Node;
	};

	function toCyNode(n: main.Node): CyNode {
		return {
			id: n.id,
			label: n.name,
			data: { ...n }
		};
	}

	function toCyEdge(e: main.Edge) {
		return {
			data: {
				source: e.sourceId,
				target: e.targetId,
				...e
			}
		};
	}

	onMount(() => {
		cy = cytoscape({
			container: graphContainer, // container to render in
			elements: {
				nodes: nodes,
				// nodes: data.nodes,
				// edges: edges_for_layout
				edges: edges
			},
			style: getGraphStyle(),
			layout: layout,
			wheelSensitivity: 0.1
		});
		// cy.expandCollapse(expandCollapseOptions);
		// `unselect` handler must be registered first because it resets selectedNode (in case nothing is selected anymore)
		cy.on('unselect', resetSelection);
		cy.on('select', handleSelection);

		store.graph((value) => {
			cy.invalidateDimensions();
			const graph = value as main.Graph;
			if (Object.keys(graph).length > 0) {
				cy.json({
					elements: { nodes: graph.nodes.map(toCyNode), edges: graph.edges.map(toCyEdge) }
				});

				// update the currently selected node
				if (selectedNode !== undefined) {
					console.log('Selected node: ', selectedNode);
					const el = graph.nodes.find((n) => n.id === selectedNodeId);
					selectedNode = el;
				}
			}

			cy.layout(layout).run();
			cy.zoom(2); // set a reasonable initial zoom
		});

		store.addSubgraph((value) => {
			const subgraph = value as main.Graph;
			if (Object.keys(subgraph).length > 0) {
				console.log(`add subgraph:`);
				console.log(subgraph);
				cy.json({
					elements: { nodes: subgraph.nodes.map(toCyNode), edges: subgraph.edges.map(toCyEdge) }
				});

				// for (let n of subgraph.nodes) {

				// 	cy.add({
				// 		group: 'nodes',
				// 		data: {
				// 			...n
				// 		}
				// 	});
				// }

				// for (let e of subgraph.edges) {
				// 	cy.add({
				// 		group: 'edges',
				// 		data: {
				// 			name: e.name,
				// 			source: e.source,
				// 			target: e.destination
				// 		}
				// 	});
				// }
			}
		});

		store.removeSubgraph((subgraph) => {
			if (Object.keys(subgraph).length > 0) {
				console.log(`removing subgraph:`);

				for (let n of subgraph.nodes) {
					console.log(n);
					// cy.add({
					// 	data: {
					// 		id: n['name'],
					// 		name: n['name']
					// 	}
					// });
				}

				for (let e of subgraph.edges) {
					// cy.add({
					// 	data: {
					// 		source: e.source,
					// 		target: e.destination
					// 	}
					// });
				}
			}
		});

		// GetGraph();
	});
</script>

<div
	id="graph"
	class={['bg-tertiary-surface-800-200', 'border-1', 'border-solid', className]}
	bind:this={graphContainer}
></div>

<style>
	#graph {
		width: 100%;
		/* height: 1000px; */
		display: block;
		/* background-color: #1a1a1a; */
	}
</style>
