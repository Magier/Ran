<script lang="ts">
	import { onMount, onDestroy, getContext } from 'svelte';
	import { browser } from '$app/environment';
	import cytoscape from 'cytoscape';
	import fcose from 'cytoscape-fcose';
	import { toaster } from '$lib/components/toaster';

	import { getGraphStyle, layout, createLayout, applyCompromisedStyle } from './graph_style';
	import type { Node, Edge } from '$lib/api/index';
	import { getCampaignState } from '$lib/components/CampaignState.svelte';
	import GraphNodeSelector from './graph_node_selector.svelte';
	// import { hierarchyLayout } from './hierachical_layout';
	// import 	{ K8sAttackGraphLayout } from './layout_claude';

	type GraphProps = {
		class?: string;
		selectedObjectId: string;
		selectedObject?: Node | Edge | undefined;
	};

	type CyNode = {
		id: string;
		label: string;
		data: Node;
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
	let searchOpen = $state(false);

	cytoscape.use(fcose);
	// cytoscape("layout", "hierarchyFlow", hierarchyLayout);
    // cytoscape('layout', 'claude', K8sAttackGraphLayout);

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
	let previousNodeIds: Set<string> = new Set();

	const theme: { isDark: boolean } = getContext('theme');

	// Re-apply cytoscape styles when theme changes
	$effect(() => {
		const isDark = theme.isDark;
		if (cy) {
			const textColor = isDark ? 'white' : 'black';
			cy.nodes().style('color', textColor);
			cy.edges().style('color', textColor);
		}
	});

	onMount(() => {
		if (browser) {
			window.addEventListener('keydown', handleKeyPress);
		}
		positions = loadPositions();
		zoom = getZoomLevelOrDefault(2);
		const prevPan = getPanPositionOrDefault(undefined);
		cy = cytoscape({
			container: graphContainer, // container to render in
			elements: {
				nodes: nodes,
				edges: edges
			},
			style: getGraphStyle(theme.isDark),
			layout: layout,
			zoom: zoom,
			wheelSensitivity: 0.1
		});
		if (prevPan) {
			cy.pan(prevPan);
		}

		// cy.expandCollapse(expandCollapseOptions);
		// `unselect` handler must be registered first because it resets selectedNode (in case nothing is selected anymore)
		cy.on('unselect', resetSelection);
		cy.on('select', handleSelection);

		cy.on('dragfree', 'node', savePositions);
		cy.on('pan', (e) => {
			pan = e.target.pan();
			sessionStorage.setItem(PAN_KEY, JSON.stringify(pan));
		});
		cy.on('scrollzoom', saveZoom);
		cy.on('pinchzoom', saveZoom);

		console.info("Cytoscape graph initialized");
		// Note: Initial centering and layout will happen when graph data loads in the $effect
	});

	onDestroy(() => {
		saveGraphLayout();
		if (browser) {
			window.removeEventListener('keydown', handleKeyPress);
		}
	});

	// reset any stored layouting information when the campaign is reset
	$effect(() => {
		if (campaignState.campaignId > 0) {
			sessionStorage.clear();
			positions = {};
			previousNodeIds.clear();
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

					// Check if there are new nodes
					const currentNodeIds = new Set(graph.nodes.map(n => n.id));
					const hasNewNodes = graph.nodes.some(n => !previousNodeIds.has(n.id));
					const hasFewerNodes = previousNodeIds.size > currentNodeIds.size;

					cy.json({
						elements: {
							nodes: nodes,
							edges: edges
						}
					});

					// Only re-layout if there are new nodes or nodes were removed
					if (hasNewNodes || hasFewerNodes || previousNodeIds.size === 0) {
						console.log(`Graph changed: ${hasNewNodes ? 'new nodes added' : hasFewerNodes ? 'nodes removed' : 'initial load'}`);

						// Lock nodes that have saved positions (they shouldn't move)
						existingNodes = cy.nodes().filter((n) => positions.hasOwnProperty(n.id()));
						existingNodes.lock();

						// Save current pan/zoom before layout
						const currentPan = cy.pan();
						const currentZoom = cy.zoom();

						try {
						// Validate graph state before layout
						const nodeCount = cy.nodes().length;
						const edgeCount = cy.edges().length;
						
						if (nodeCount === 0) {
							console.warn('Skipping layout: no nodes in graph');
							return;
						}

						// Create layout with constraints and add stop callback to unlock nodes
						const enhancedLayout = createLayout(cy.nodes(), positions);

						// Validate layout configuration
						console.log('Layout config:', enhancedLayout);
						console.log('Graph state:', { nodes: nodeCount, edges: edgeCount });
							const originalStop = enhancedLayout.stop;
					const isInitialLoad = previousNodeIds.size === 0;
					enhancedLayout.stop = () => {
						existingNodes.unlock();

						if (isInitialLoad) {
							// Center and fit on initial load with reasonable zoom cap
							console.log('Initial load: centering graph');
							cy.fit(undefined, 50); // 50px padding to fit all nodes
							
							// Cap zoom to avoid being too zoomed in
							const maxZoom = 2;
							if (cy.zoom() > maxZoom) {
								cy.zoom(maxZoom);
							}
							cy.center();
						} else {
							// Restore pan and zoom for subsequent updates
							if (currentPan && (currentPan.x !== 0 || currentPan.y !== 0)) {
								cy.pan(currentPan);
							}
							cy.zoom(currentZoom);
						}
								console.log('Layout complete, nodes unlocked');
								if (originalStop) originalStop();
							};

							cy.layout(enhancedLayout).run();

							// Update tracking
							previousNodeIds = currentNodeIds;
						} catch (layoutError) {
							console.error('Layout error:', layoutError);
							console.error('Node count:', cy.nodes().length);
						console.error('Edge count:', cy.edges().length);
						console.error('Positions:', positions);
						existingNodes.unlock();
						
						// Clear invalid positions on error to allow fresh layout
						const errorMsg = layoutError instanceof Error ? layoutError.message : String(layoutError);
						if (errorMsg.includes('invalid array length')) {
							console.warn('Clearing invalid positions due to array length error');
							sessionStorage.removeItem(POS_KEY);
							positions = {};
						}
						
						toaster.create({ 
							title: "Layout error", 
							description: 'Error during layout: ' + errorMsg, 
							type: 'error' 
						});
						}
					} else {
						console.log('No new nodes, skipping layout');
					}

					applyCompromisedStyle(cy);

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

	function savePositions() {
		if (cy === undefined) {
			console.error("Cytoscape instance is undefined, cannot save positions");
			return;
		}
		const map: PosMap = {};
		cy.nodes().forEach(n => { map[n.id()] = n.position(); });
		positions = map;
		sessionStorage.setItem(POS_KEY, JSON.stringify(positions));

	};
	function saveZoom() {
		if (browser) {
			sessionStorage.setItem(ZOOM_KEY, JSON.stringify(cy.zoom()));
		}
	};

	function saveGraphLayout() {
		if (browser) {
			if (cy === undefined) {
				console.error("Cytoscape instance is undefined, cannot save graph layout");
				return;
			}
			console.log("Saving graph layout");
			savePositions();
			saveZoom();
			sessionStorage.setItem(PAN_KEY, JSON.stringify(cy.pan()));
		}
	};

	function loadPositions(): PosMap {
		if (!browser) return {};
		try { 
			const stored = JSON.parse(sessionStorage.getItem(POS_KEY) ?? '{}');
			// Validate loaded positions
			const validated: PosMap = {};
			for (const [id, pos] of Object.entries(stored)) {
				if (pos && typeof pos === 'object') {
					const { x, y } = pos as Pos;
					if (typeof x === 'number' && typeof y === 'number' &&
						isFinite(x) && isFinite(y) &&
						!isNaN(x) && !isNaN(y) &&
						Math.abs(x) < 1e6 && Math.abs(y) < 1e6) {
						validated[id] = { x, y };
					} else {
						console.warn(`Invalid position for node ${id}, skipping`);
					}
				}
			}
			return validated;
		}
		catch (e) { 
			console.error('Error loading positions:', e);
			return {}; 
		}
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

	function toCyNode(n: Node, nodePos: Record<string, any>): CyNode {
		let cyNode: CyNode = {
			id: n.id,
			label: n.name,
			data: { ...n }
		};

		if (nodePos.hasOwnProperty(n.id)) {
			cyNode.position = nodePos[n.id];
		}

		return cyNode;
	}

	function toCyEdge(e: Edge) {
		return {
			data: {
				source: e.sourceId,
				target: e.targetId,
				...e
			}
		};
	}

	function handleKeyPress(event: KeyboardEvent) {
		// Only trigger if not typing in an input/textarea and search is not already open
		if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement) {
			return;
		}

		if (event.key === 't' && !searchOpen) {
			event.preventDefault();
			openSearch();
		}
	}

	function openSearch() {
		searchOpen = true;
	}

</script>

<div id="graph" class={['bg-tertiary-surface-800-200', className]} bind:this={graphContainer}></div>

<GraphNodeSelector {cy} bind:isOpen={searchOpen} />

<style>
	#graph {
		width: 100%;
		/* height: 1000px; */
		display: block;
		/* background-color: #1a1a1a; */
	}
</style>
