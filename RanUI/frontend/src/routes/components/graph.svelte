<script lang="ts">
	import { onMount } from 'svelte';
	import cytoscape from 'cytoscape';
	import dagre from 'cytoscape-dagre';
	import { toaster } from '$lib/components/toaster';

	import { getGraphStyle, layout } from './graph_style';
	import store from '$lib/stores/store.js';
	import type { main } from '$lib/wailsjs/go/models';
	import { getCampaignState } from '$lib/components/CampaignState.svelte';

	type GraphProps = {
		class?: string;
		selectedObjectId: string;
		selectedObject?: main.Node | main.Edge | undefined;
	};
	let {
		class: className = '',
		selectedObjectId = $bindable(),
		selectedObject = $bindable()
	}: GraphProps = $props();

	let nodes = $state([]);
	let edges = $state([]);

	let graphContainer = $state();

	cytoscape.use(dagre);

	function handleSelection(event: cytoscape.Event) {
		let el = event.target;
		selectedObject = el.data();
		selectedObjectId = el.data()['id'];
		console.group("Selected Graph object");
		console.log(el.data());
		console.log(el.classes());
		console.groupEnd();
	}

	function resetSelection(event: cytoscape.Event) {
		selectedObject = undefined;
		selectedObjectId = '';
	}

	type CyNode = {
		id: string;
		label: string;
		data: main.Node;
		position?: { x: number; y: number };
	};

	function toCyNode(n: main.Node, nodePos: Record<string, any>): CyNode {
		let cyNode: CyNode ={
			id: n.id,
			label: n.name,
			data: { ...n }
		}

		if (nodePos.hasOwnProperty(n.id)) {
			cyNode.position = nodePos[n.id];
		}

		return cyNode
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

	let cy: cytoscape.Core = cytoscape({
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

	const campaignState = getCampaignState();

	$effect(() => {
		cy.invalidateDimensions();
		const graph = campaignState.graph;
		if (Object.keys(graph).length > 0) {
			let nodePos: Record<string, any> = {};

			cy.nodes().forEach((n) => {
				nodePos[n.id()] = n.position();
			});

			try {
				if (graph.nodes === undefined || graph.edges === undefined) {
					console.warn('Graph data is incomplete:', graph);

				} else {
					let nodes = graph.nodes.map(n => toCyNode(n, nodePos)); 
					let edges = graph.edges.map(toCyEdge);

					cy.json({
						elements: {
							nodes: nodes,
							edges: edges
						}
					});
				}
			} catch (e) {
				console.error('Error updating graph:', e);
				toaster.create({ title: "Graph error", description: 'Error updating graph: ' + e, type: 'error' });
			}

			// update the currently selected graph object
			if (selectedObject !== undefined) {
				if (selectedObject.entity !== undefined) {
					const el = graph.nodes.find((n) => n.id === selectedObjectId);
					selectedObject = el;
				} else {
					const el = graph.edges.find((n) => n.id === selectedObjectId);
					selectedObject = el;
				}

			}
		}

		cy.layout(layout).run();
		cy.zoom(2); // set a reasonable initial zoom
	});

	onMount(() => {

		// store.graph((value) => {
		// 	cy.invalidateDimensions();
		// 	const graph = value as main.Graph;
		// 	if (Object.keys(graph).length > 0) {
		// 		let nodePos: Record<string, any> = {};

		// 		cy.nodes().forEach((n) => {
		// 			nodePos[n.id()] = n.position();
		// 		});

		// 		try {
		// 			cy.json({
		// 				elements: {
		// 					nodes: graph.nodes.map(n => toCyNode(n, nodePos)), 
		// 					edges: graph.edges.map(toCyEdge),
		// 				}
		// 			});
		// 		} catch (e) {
		// 			toaster.create({ title: "Graph error", description: 'Error updating graph: ' + e, type: 'error' });
		// 		}

		// 		// update the currently selected graph object
		// 		if (selectedObject !== undefined) {
		// 			if (selectedObject.entity !== undefined) {
		// 				const el = graph.nodes.find((n) => n.id === selectedObjectId);
		// 				selectedObject = el;
		// 			} else {
		// 				const el = graph.edges.find((n) => n.id === selectedObjectId);
		// 				selectedObject = el;
		// 			}

		// 		}
		// 	}

		// 	cy.layout(layout).run();
		// 	cy.zoom(2); // set a reasonable initial zoom
		// });

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

<div id="graph" class={['bg-tertiary-surface-800-200', className]} bind:this={graphContainer}></div>

<style>
	#graph {
		width: 100%;
		/* height: 1000px; */
		display: block;
		/* background-color: #1a1a1a; */
	}
</style>
