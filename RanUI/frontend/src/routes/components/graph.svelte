<script lang="ts">
	import { onMount } from 'svelte';
	import { browser } from '$app/environment';
	import cytoscape from 'cytoscape';
	import fcose from 'cytoscape-fcose';
	import { toaster } from '$lib/components/toaster';

	import { getGraphStyle, layout } from './graph_style';
	import type { main } from '$lib/wailsjs/go/models';
	import { getCampaignState } from '$lib/components/CampaignState.svelte';

	type GraphProps = {
		class?: string;
		selectedObjectId: string;
		selectedObject?: main.Node | main.Edge | undefined;
	};

	type CyNode = {
		id: string;
		label: string;
		data: main.Node;
		position?: { x: number; y: number };
	};
	type Pos = { x: number; y: number };
	type PosMap = Record<string, Pos>;

	let {
		class: className = '',
		selectedObjectId = $bindable(),
		selectedObject = $bindable()
	}: GraphProps = $props();

	let nodes = $state([]);
	let edges = $state([]);

	cytoscape.use(fcose);

	let cy: cytoscape.Core;
	let graphContainer: HTMLElement;
	let positions: PosMap = {};
	let zoom: number = 1;
	let pan: Pos = { x: 0, y: 0 };
	const campaignState = getCampaignState();
	// let positions: PosMap = $state({});
	const POS_KEY = 'nodePositions';
	const PAN_KEY = '_pan';
	const ZOOM_KEY = '_zoom';

	// keep track of the nodes before the layouting, to ensure consistent positioning of new nodes
	let existingNodes: cytoscape.NodeCollection = cytoscape().collection();

	onMount(() => {
		positions = loadPositions();
		zoom = getZoomLevelOrDefault(2);
		pan = getPanPositionOrDefault({ x: 0, y: 0 });

		cy = cytoscape({
			container: graphContainer, // container to render in
			elements: {
				nodes: nodes,
				edges: edges
			},
			style: getGraphStyle(),
			layout: layout,
			pan: pan,
			zoom: zoom,
			wheelSensitivity: 0.1
		});

		layout.stop = () => {
			existingNodes.unlock();
			// apply previous pan and zoom, to nullify side-effects from layout
			cy.pan(getPanPositionOrDefault({ x: 0, y: 0 }));
			cy.zoom(getZoomLevelOrDefault(2));
		};


		// cy.expandCollapse(expandCollapseOptions);
		// `unselect` handler must be registered first because it resets selectedNode (in case nothing is selected anymore)
		cy.on('unselect', resetSelection);
		cy.on('select', handleSelection);

		const savePositions = () => {
			const map: PosMap = {};
			cy.nodes().forEach(n => { map[n.id()] = n.position(); });
			positions = map;
			sessionStorage.setItem(POS_KEY, JSON.stringify(positions));
		};

		const saveZoom = () => {
			if (browser) {
				sessionStorage.setItem(ZOOM_KEY, JSON.stringify(cy.zoom()));
			}
		};

		const saveGraphLayout = () => {
			if (browser) {
				savePositions();
				saveZoom();
				sessionStorage.setItem(PAN_KEY, JSON.stringify(cy.pan()));
			}
		};

		cy.on('dragfree', 'node', savePositions);
		cy.on('pan', (e) => {
			pan = e.target.pan();
			sessionStorage.setItem(PAN_KEY, JSON.stringify(pan));
		});
		cy.on('scrollzoom', saveZoom);
		cy.on('pinchzoom', saveZoom);

		if (Object.keys(positions).length === 0) {
			console.info("No previous positions, so using sane defaults for the layout")
			cy.layout(layout).run();
			cy.center();
			saveGraphLayout();
		}
	});

	// reset any stored layouting information when the campaign is reset
	$effect(() => {
		if (campaignState.campaignId > 0) {
			sessionStorage.clear();
		}
	});

	$effect(() => {
		cy.invalidateDimensions();
		const graph = campaignState.graph;

		if (Object.keys(graph).length > 0) {
			try {
				if (graph.nodes === undefined) {
					console.warn('Graph data is incomplete:', graph);
				} else {
					let nodes = graph.nodes.map(n => toCyNode(n, positions)); 
					let edges = graph.edges.map(toCyEdge);

					cy.json({
						elements: {
							nodes: nodes,
							edges: edges
						}
					});

					existingNodes = cy.nodes().filter((n) => n.id() in positions);
					existingNodes.lock();  // layout.stop function takes care of unlocking the nodes after laying the new ones out
					cy.layout(layout).run();

					// use timeout 0 to not track selectedObject as a dependency
					setTimeout(() => {
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
					})
				}
			} catch (e) {
				console.error('Error updating graph:', e);
				toaster.create({ title: "Graph error", description: 'Error updating graph: ' + e, type: 'error' });
			}
		}
	});

	function loadPositions(): PosMap {
		if (!browser) return {};
		try { 
			return JSON.parse(sessionStorage.getItem(POS_KEY) ?? '{}') 
		}
		catch { return {}; }
	}	

	function getZoomLevelOrDefault(defaultValue: number) {
		if (browser) {
			const zoom = sessionStorage.getItem(ZOOM_KEY);
			return zoom ? JSON.parse(zoom) : defaultValue;
		}
		return defaultValue;
	}

	function getPanPositionOrDefault(defaultValue: Pos): Pos {
		if (browser) {
			const pan = sessionStorage.getItem(PAN_KEY);
			return pan ? JSON.parse(pan) : defaultValue;
		}
		return defaultValue;
	}

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
