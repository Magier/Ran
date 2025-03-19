<script lang="ts">
	import { onMount } from 'svelte';
	import cytoscape from 'cytoscape';
	import fcose from 'cytoscape-fcose';
	import * as runtime from '$lib/wailsjs/runtime';
	import { GetGraph } from '$lib/wailsjs/go/main/App';
	import cytoscapePopper from 'cytoscape-popper';
	import {
		computePosition,
		arrow,
		autoPlacement,
		shift,
		limitShift,
		offset
	} from '@floating-ui/dom';

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
	let arrowEl = $state();
	let currPopper = $state();

	function popperFactory(ref, content, opts) {
		// see https://floating-ui.com/docs/computePosition#options
		const popperOptions = {
			// matching the default behaviour from Popper@2
			// 	// https://floating-ui.com/docs/migration#configure-middleware
			middleware: [
				// flip(),
				// shift({limiter: limitShift()}),
				autoPlacement({ crossAxis: true }),
				offset({
					mainAxis: 8, // 2xoffset for the arrow
					crossAxis: -graphContainer?.getBoundingClientRect().top
				}),
				// https://floating-ui.com/docs/arrow
				arrow({ element: arrowEl })
				// arrow({ element: document.querySelector('#arrow') }),
			],
			...opts
		};

		function update() {
			computePosition(ref, content, popperOptions).then(({ x, y, placement, middlewareData }) => {
				// TODO: if element/ arrowEl are not set (because of rerender), then query them via DOM again

				Object.assign(content.style, {
					left: `${x}px`,
					top: `${y}px`
				});

				// if (arrowEl) {
				// 	const { x: arrowX, y: arrowY } = middlewareData.arrow;

				// 	console.log(`placement: ${placement}/ X: ${arrowX}, Y :${arrowY}`);
				// 	// copied from skeleton repo ./src/lib/utilitis/Popup/popup.ts
				// 	const staticSide: string = {
				// 		top: 'bottom',
				// 		right: 'left',
				// 		bottom: 'top',
				// 		left: 'right'
				// 	}[placement.split('-')[0]];

				// 	Object.assign(arrowEl?.style, {
				// 		left: arrowX != null ? `${arrowX}px` : '',
				// 		// top: arrowY != null ? `${arrowY}px` : '',
				// 		top: arrowY != null ? '50%' : '',
				// 		[staticSide]: '-4px'
				// 	});
				// } else {
				// 	console.error('arrowElement for the popup not defined!');
				// }
			});
		}
		update();
		return { update };
	}

	cytoscape.use(fcose);
	cytoscape.use(cytoscapePopper(popperFactory));

	function updatePopup() {
		if (currPopper) {
			currPopper.update();
		}
	}

	function handleSelection(event: cytoscape.Event) {
		let el = event.target;
		currPopper = el.popper({
			content: () => {
				let div = document.querySelector('.details-popup');
				return div;
			},
			// renderedPosition: () => ({ x: 400, y: 400 }),
			popper: {
				placement: 'top'
			} // @floating-ui options (https://floating-ui.com/docs/middleware)
		});

		el.on('position', updatePopup);
		event.cy.on('pan zoom resize', updatePopup);

		selectedNode = el.data();
		selectedNodeId = el.data()['id'];
		console.log(el.data());
		console.log(el.classes());
	}

	function resetSelection(event: cytoscape.Event) {
		selectedNode = null;
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

<div id="graph" class={className} bind:this={graphContainer}>
	<!-- specify data-popup attr. for consistent styling via skeleton-ui -->
	<div
		class="card variant-filled-secondary details-popup p-4 {selectedNode ? 'show' : ''}"
		data-popup
	>
		<div><span>Name:</span> {selectedNode?.name}</div>
		{#if selectedNode?.ip}
			<div><span>IP:</span> {selectedNode?.ip}</div>{/if}
		{#if selectedNode?.kind}
			<div><span>Kind:</span> {selectedNode?.kind}</div>{/if}
		{#if selectedNode?.username}
			<div><span>Username:</span> {selectedNode?.username}</div>{/if}
		{#if selectedNode?.accessLevel}
			<div><span>AccessLevel:</span> {selectedNode?.accessLevel}</div>{/if}
		{#if selectedNode?.os}
			<div><span>OS:</span> {selectedNode?.os}</div>{/if}
		{#if selectedNode?.version}
			<div><span>Version:</span> {selectedNode?.version}</div>{/if}
		<div bind:this={arrowEl} class="arrow variant-filled"></div>
	</div>
</div>

<style>
	#graph {
		width: 100%;
		height: 1000px;
		display: block;
		background-color: #1a1a1a;
	}

	.details-popup {
		position: absolute;
		display: none;
	}
	.details-popup.show {
		display: block;
	}
</style>
