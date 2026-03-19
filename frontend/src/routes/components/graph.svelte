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
			cy.edges('[!informational]').style('color', textColor);
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

	// Track campaign ID changes and reset layout when campaign changes
	let previousCampaignId = $state(campaignState.campaignId);
	$effect(() => {
		const currentCampaignId = campaignState.campaignId;
		// Only reset if campaign actually changed (not just on mount)
		if (previousCampaignId !== currentCampaignId && previousCampaignId !== undefined) {
			console.log(`Campaign changed from ${previousCampaignId} to ${currentCampaignId}, resetting layout`);
			// Clear only graph-specific keys, not all sessionStorage
			sessionStorage.removeItem(POS_KEY);
			sessionStorage.removeItem(PAN_KEY);
			sessionStorage.removeItem(ZOOM_KEY);
			// Don't clear FILTER_NS_KEY - preserve namespace filter across campaigns
			positions = {};
			previousNodeIds.clear();
			// Don't reset hiddenNamespaces - user preferences should persist
		}
		previousCampaignId = currentCampaignId;
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
					// Clean up stale position entries before processing
					const currentNodeIds = new Set(graph.nodes.map(n => n.id));
					let positionsChanged = false;
					Object.keys(positions).forEach(id => {
						if (!currentNodeIds.has(id)) {
							delete positions[id];
							positionsChanged = true;
						}
					});

					// Persist cleaned positions immediately to avoid re-loading stale data
					if (positionsChanged && browser) {
						sessionStorage.setItem(POS_KEY, JSON.stringify(positions));
					}

					let nodes = graph.nodes.map(n => toCyNode(n, positions));
					let edges = graph.edges.map(toCyEdge);

					// Check if there are new nodes
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

					// Lock nodes that have saved positions, but exclude compound nodes with new children
					// to allow their children to be properly laid out
					existingNodes = cy.nodes().filter((n) => {
						if (!positions.hasOwnProperty(n.id())) return false;
						
						// If this is a compound node (has children), check if it has new children
						if (n.isParent()) {
							const children = n.children();
							const hasNewChild = children.some((child: any) => !previousNodeIds.has(child.id()));
							// Don't lock compound nodes with new children
							if (hasNewChild) {
								console.log(`Not locking compound node ${n.id()} - has new children`);
								return false;
							}
						}
						
						return true;
					});
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
						// Use only visible nodes (not filtered or hidden by collapse)
						const visibleNodes = cy.nodes(':visible');
						const nodeIdSet = new Set(visibleNodes.map(n => n.id()));
						
						// Validate all edges reference visible nodes
						let hasInvalidEdges = false;
						cy.edges().forEach(e => {
							const src = e.source().id();
							const tgt = e.target().id();
							if (!nodeIdSet.has(src) || !nodeIdSet.has(tgt)) {
								console.warn(`Edge "${e.id()}" references hidden or non-existent node (${src} -> ${tgt})`);
								hasInvalidEdges = true;
							}
						});

						if (hasInvalidEdges) {
							console.warn('Skipping layout due to invalid edges');
							existingNodes.unlock();
							return;
						}

						// Assert: container must have non-zero dimensions
						const containerRect = graphContainer.getBoundingClientRect();
						if (containerRect.width === 0 || containerRect.height === 0) {
							throw new Error(`Graph container has zero dimensions (${containerRect.width}x${containerRect.height})`);
						}

						// Create layout with constraints using only visible nodes
						// This prevents the layout from trying to process collapsed/filtered nodes
						const enhancedLayout = createLayout(visibleNodes, positions);

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
						
						// Save positions after layout completes to preserve them for future updates
						savePositions();
						
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

					// Reapply text color based on current theme
					const textColor = theme.isDark ? 'white' : 'black';
					cy.nodes().style('color', textColor);
					cy.edges('[!informational]').style('color', textColor);

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
		} else {
			// Set default positions for initial nodes and save them to positions
			if (n.name === 'Ran' || n.id === 'c2/Ran') {
				cyNode.position = { x: -100, y: 0 };
				nodePos[n.id] = cyNode.position;
			}
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
		// After the expand-collapse plugin has collapsed this node, consolidate
		// all visible edges between the same directed pair (compound node <-> external node)
		// into a single meta-edge. The plugin may have created per-type meta-edges;
		// we merge those further so only one edge per direction per external node remains.

		const connectedEdges = node.connectedEdges().filter((e: any) => e.visible());

		// Group by directed source->target pair
		const edgeGroups = new Map<string, any[]>();

		connectedEdges.forEach((edge: any) => {
			const sourceId = edge.source().id();
			const targetId = edge.target().id();
			if (sourceId === targetId) return; // skip self-loops
			const key = `${sourceId}->${targetId}`;
			if (!edgeGroups.has(key)) {
				edgeGroups.set(key, []);
			}
			edgeGroups.get(key)!.push(edge);
		});

		// Consolidate groups with multiple edges into a single meta-edge
		edgeGroups.forEach((edges, key) => {
			if (edges.length <= 1) return; // single edge, nothing to consolidate

			const separator = '->';
			const sepIndex = key.indexOf(separator);
			const sourceId = key.substring(0, sepIndex);
			const targetId = key.substring(sepIndex + separator.length);
			const metaEdgeId = `meta-${sourceId}-to-${targetId}`;

			// Remove a prior meta-edge for this pair if it exists
			const existing = cy.getElementById(metaEdgeId);
			if (existing.length > 0) existing.remove();

			// Hide all edges in this group
			edges.forEach((e: any) => e.hide());

			// Build a descriptive label from unique edge names
			const uniqueNames = [...new Set(edges.map((e: any) => e.data('name')))].filter(Boolean);
			const label = uniqueNames.length === 1 ? uniqueNames[0] : `${edges.length} relations`;

			cy.add({
				group: 'edges',
				data: {
					id: metaEdgeId,
					source: sourceId,
					target: targetId,
					name: label,
					collapsedEdges: edges.map((e: any) => e.id()),
					isMetaEdge: true
				}
			});
		});
	}

	/**
	 * Hide informational edges between a node pair when a non-informational
	 * (actionable/factual) edge already exists for that same pair in the same direction.
	 * Skips edges that are already hidden by the namespace filter.
	 */
	function hideRedundantInformationalEdges(cy: cytoscape.Core) {
		// Collect directed node-pairs that have at least one non-informational, non-filtered edge
		const hasActionableEdge = new Set<string>();
		cy.edges().forEach(e => {
			if (!e.data('informational') && !e.hasClass('namespace-filtered')) {
				// Use a directed key: source->target (order matters)
				const pair = `${e.source().id()}->${e.target().id()}`;
				hasActionableEdge.add(pair);
			}
		});

		// Hide informational edges whose directed pair has an actionable edge
		cy.edges('[?informational]').forEach(e => {
			if (e.hasClass('namespace-filtered')) return; // don't touch namespace-filtered edges
			const pair = `${e.source().id()}->${e.target().id()}`;
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
		// Remove all our custom meta-edges related to this node and restore
		// the edges we hid (the plugin restores its own internal state).
		cy.edges('[?isMetaEdge]').forEach((metaEdge: any) => {
			const source = metaEdge.source().id();
			const target = metaEdge.target().id();

			if (source === node.id() || target === node.id()) {
				const collapsedEdgeIds: string[] = metaEdge.data('collapsedEdges') || [];

				// Show back the edges we hid
				collapsedEdgeIds.forEach((edgeId: string) => {
					const edge = cy.getElementById(edgeId);
					if (edge.length > 0) edge.show();
				});

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
