<script lang="ts">
	import { onMount, onDestroy, getContext } from 'svelte';
	import { browser } from '$app/environment';
	import cytoscape from 'cytoscape';
	import fcose from 'cytoscape-fcose';
	// @ts-ignore
	import expandCollapse from 'cytoscape-expand-collapse';
	import { toaster } from '$lib/components/toaster';

	import { getGraphStyle, layout, createLayout, applyCompromisedStyle } from './graph_style';
	import { isInformational } from './edge_categories';
	import type { Node, Edge } from '$lib/api/index';
	import { getCampaignState } from '$lib/components/CampaignState.svelte';
	import GraphNodeSelector from './graph_node_selector.svelte';
	import GraphFilter from './graph_filter.svelte';
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

	const FILTER_NS_KEY = '_hiddenNamespaces';
	const DEFAULT_HIDDEN_NAMESPACES = ['kube-system', 'local-path-storage'];

	function loadHiddenNamespaces(): Set<string> {
		if (!browser) return new Set(DEFAULT_HIDDEN_NAMESPACES);
		try {
			const stored = sessionStorage.getItem(FILTER_NS_KEY);
			return stored ? new Set(JSON.parse(stored)) : new Set(DEFAULT_HIDDEN_NAMESPACES);
		} catch {
			return new Set(DEFAULT_HIDDEN_NAMESPACES);
		}
	}

	let hiddenNamespaces: Set<string> = $state(loadHiddenNamespaces());

	cytoscape.use(fcose);
	if (typeof expandCollapse === 'function') {
		cytoscape.use(expandCollapse);
	} else if (expandCollapse?.default) {
		cytoscape.use(expandCollapse.default);
	}

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

	// Derive available namespaces (compound nodes that have children) from graph data
	const availableNamespaces = $derived.by(() => {
		const graph = campaignState.graph;
		if (!graph?.nodes) return [];
		const parentIds = new Set(graph.nodes.filter((n) => n.parent).map((n) => n.parent!));
		return graph.nodes.filter((n) => parentIds.has(n.id)).map((n) => n.name);
	});
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

		// Initialize expand-collapse extension
		let api;
		try {
			api = cy.expandCollapse({
				layoutBy: null,
				fisheye: false,
				animate: true,
				animationDuration: 300,
				undoable: false,
				cueEnabled: true,
				expandCollapseCuePosition: 'top-left',
				expandCollapseCueSize: 12,
				expandCollapseCueLineSize: 8,
				expandCollapseCueSensitivity: 1,
				allowNestedEdgeCollapse: true,
				edgeTypeInfo: "name",
				groupEdgesOfSameTypeOnCollapse: true,
				zIndex: 999
			});
		} catch (error) {
			console.error('Error initializing expand-collapse:', error);
			api = null;
		}

		// Make API available for the graph update effect (expand/re-collapse on update)
		if (browser) {
			(window as any).cyExpandCollapseAPI = api;
		}

		cy.on('expandcollapse.aftercollapse', (event) => {
			handleAfterCollapse(event.target);
		});

		cy.on('expandcollapse.afterexpand', (event) => {
			handleAfterExpand(event.target);
		});

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
			hiddenNamespaces = new Set(DEFAULT_HIDDEN_NAMESPACES);
		}
	});

	// Persist hidden namespaces to sessionStorage
	$effect(() => {
		if (browser) {
			sessionStorage.setItem(FILTER_NS_KEY, JSON.stringify([...hiddenNamespaces]));
		}
		// Re-apply filter whenever hidden namespaces change
		if (cy) {
			applyNamespaceFilters(cy, hiddenNamespaces);
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

					// Expand all collapsed nodes before updating to avoid duplicate node errors.
					// The expand-collapse plugin hides children internally, and cy.json()
					// would try to re-add them, causing duplicates.
					const collapsedNodes: string[] = [];
					const ecApi = (window as any).cyExpandCollapseAPI;
					if (ecApi) {
						cy.nodes('.cy-expand-collapse-collapsed-node').forEach((n: any) => {
							collapsedNodes.push(n.id());
							try { ecApi.expand(n); } catch (_) {}
						});
					}

					cy.json({
						elements: {
							nodes: nodes,
							edges: edges
						}
					});

					// Re-collapse nodes that were previously collapsed
					if (ecApi && collapsedNodes.length > 0) {
						collapsedNodes.forEach(id => {
							const node = cy.getElementById(id);
							if (node.length > 0 && node.isParent()) {
								try { ecApi.collapse(node); } catch (_) {}
							}
						});
					}

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

						// Assert: all edges must reference existing nodes
						const nodeIdSet = new Set(cy.nodes().map(n => n.id()));
						cy.edges().forEach(e => {
							const src = e.source().id();
							const tgt = e.target().id();
							if (!nodeIdSet.has(src)) {
								throw new Error(`Edge "${e.id()}" references non-existent source node "${src}"`);
							}
							if (!nodeIdSet.has(tgt)) {
								throw new Error(`Edge "${e.id()}" references non-existent target node "${tgt}"`);
							}
						});

						// Assert: container must have non-zero dimensions
						const containerRect = graphContainer.getBoundingClientRect();
						if (containerRect.width === 0 || containerRect.height === 0) {
							throw new Error(`Graph container has zero dimensions (${containerRect.width}x${containerRect.height})`);
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
					applyNamespaceFilters(cy, hiddenNamespaces);

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

		// Add parent relationship if exists (required for expand-collapse)
		if (n.parent) {
			cyNode.data.parent = n.parent;
		}

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
				...e,
				informational: isInformational(e.name)
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

	function handleAfterCollapse(node: any) {
		// Get all descendant nodes (children/grandchildren) of the collapsed node
		const descendants = node.descendants();
		// Group edges between the collapsed node and external nodes
		const edgeGroups = new Map(); // Key: "sourceId-targetId", Value: array of edges
		
		// Find all edges that connect descendants to external nodes
		descendants.forEach((desc: any) => {
			// Outgoing edges from descendants to external nodes
			desc.connectedEdges().forEach((edge: any) => {
				const source = edge.source();
				const target = edge.target();
				
				// Check if this is an edge going out of the collapsed group
				if (descendants.contains(source) && !descendants.contains(target) && target.id() !== node.id()) {
					const key = `${node.id()}->${target.id()}`;
					if (!edgeGroups.has(key)) {
						edgeGroups.set(key, []);
					}
					edgeGroups.get(key).push(edge);
				}
				// Check if this is an edge coming into the collapsed group
				else if (!descendants.contains(source) && descendants.contains(target) && source.id() !== node.id()) {
					const key = `${source.id()}->${node.id()}`;
					if (!edgeGroups.has(key)) {
						edgeGroups.set(key, []);
					}
					edgeGroups.get(key).push(edge);
				}
			});
		});
		
		// Create or update meta-edges for each group
		edgeGroups.forEach((edges, key) => {
			if (edges.length === 0) return;
			
			// Key format is "sourceId->targetId" to handle IDs with dashes
			const separator = '->';
			const sepIndex = key.indexOf(separator);
			const sourceId = key.substring(0, sepIndex);
			const targetId = key.substring(sepIndex + separator.length);
			const metaEdgeId = `meta-${sourceId}-to-${targetId}`;
			
			// Remove existing meta-edge if it exists
			const existingMetaEdge = cy.getElementById(metaEdgeId);
			if (existingMetaEdge.length > 0) {
				existingMetaEdge.remove();
			}
			
			// Hide the original edges
			edges.forEach((e: any) => e.hide());
			
			// Create a new meta-edge
			cy.add({
				group: 'edges',
				data: {
					id: metaEdgeId,
					source: sourceId,
					target: targetId,
					name: edges.length > 1 ? `${edges.length} relations` : edges[0].data('name'),
					collapsedEdges: edges.map((e: any) => e.id()),
					isMetaEdge: true
				}
			});
			
		});
	}

	/**
	 * Hide informational edges between a node pair when a non-informational
	 * (actionable/factual) edge already exists for that same pair.
	 * Skips edges that are already hidden by the namespace filter.
	 */
	function hideRedundantInformationalEdges(cy: cytoscape.Core) {
		// Collect node-pairs that have at least one non-informational, non-filtered edge
		const hasActionableEdge = new Set<string>();
		cy.edges().forEach(e => {
			if (!e.data('informational') && !e.hasClass('namespace-filtered')) {
				// Use an unordered key so A->B and B->A share the same pair
				const pair = [e.source().id(), e.target().id()].sort().join('||');
				hasActionableEdge.add(pair);
			}
		});

		// Hide informational edges whose pair has an actionable edge
		cy.edges('[?informational]').forEach(e => {
			if (e.hasClass('namespace-filtered')) return; // don't touch namespace-filtered edges
			const pair = [e.source().id(), e.target().id()].sort().join('||');
			if (hasActionableEdge.has(pair)) {
				e.hide();
			} else {
				e.show();
			}
		});
	}

	/**
	 * Hide nodes (and their edges) belonging to the specified namespaces.
	 * Uses the 'namespace-filtered' class to track which elements were hidden
	 * by this filter, so other hide/show logic isn't affected.
	 */
	function applyNamespaceFilters(cy: cytoscape.Core, hidden: Set<string>) {
		// Step 1: restore elements previously hidden by this filter
		cy.elements('.namespace-filtered').forEach((el: any) => {
			el.removeClass('namespace-filtered');
			if (el.isNode()) {
				// Don't re-show if it's a child of a collapsed compound node
				const parent = el.parent();
				const isCollapsedChild =
					parent.length > 0 && parent.hasClass('cy-expand-collapse-collapsed-node');
				if (!isCollapsedChild) {
					el.show();
				}
			} else if (!el.data('isMetaEdge')) {
				el.show();
			}
		});

		if (hidden.size === 0) {
			// No filter — re-apply informational edge logic and return
			hideRedundantInformationalEdges(cy);
			return;
		}

		// Step 2: collect node IDs that belong to filtered namespaces
		const filteredNodeIds = new Set<string>();
		hidden.forEach((nsName) => {
			cy.nodes().forEach((n: any) => {
				if (n.data('name') === nsName && n.isParent()) {
					filteredNodeIds.add(n.id());
					n.descendants().forEach((d: any) => filteredNodeIds.add(d.id()));
				}
			});
		});

		// Step 3: hide filtered nodes
		filteredNodeIds.forEach((id) => {
			const n = cy.getElementById(id);
			if (n.length > 0) {
				n.addClass('namespace-filtered');
				n.hide();
			}
		});

		// Step 4: hide edges touching filtered nodes
		cy.edges().forEach((e: any) => {
			if (e.data('isMetaEdge')) return;
			if (filteredNodeIds.has(e.source().id()) || filteredNodeIds.has(e.target().id())) {
				e.addClass('namespace-filtered');
				e.hide();
			}
		});

		// Step 5: re-apply informational edge hiding on the remaining visible elements
		hideRedundantInformationalEdges(cy);
	}

	function handleAfterExpand(node: any) {
		// Remove all meta-edges related to this node and restore original edges
		cy.edges('[isMetaEdge]').forEach((metaEdge: any) => {
			const source = metaEdge.source().id();
			const target = metaEdge.target().id();
			
			// Check if this meta-edge is related to the expanded node
			if (source === node.id() || target === node.id()) {
				const collapsedEdgeIds = metaEdge.data('collapsedEdges') || [];
				
				// Show the original edges
				collapsedEdgeIds.forEach((edgeId: string) => {
					const originalEdge = cy.getElementById(edgeId);
					if (originalEdge.length > 0) {
						originalEdge.show();
					}
				});
				
				// Remove the meta-edge
				metaEdge.remove();
			}
		});
	}

</script>

<div class={['graph-wrapper', className]}>
	<div id="graph" bind:this={graphContainer}></div>
	<GraphFilter {availableNamespaces} bind:hiddenNamespaces />
</div>

<GraphNodeSelector {cy} bind:isOpen={searchOpen} />

<style>
	.graph-wrapper {
		position: relative;
		width: 100%;
		height: 100%;
	}

	#graph {
		width: 100%;
		height: 100%;
		display: block;
		position: absolute;
		inset: 0;
		background-color: var(--color-tertiary-surface-800-200, transparent);
		z-index: 0;
	}
</style>
