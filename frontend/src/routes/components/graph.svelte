<script lang="ts">
	import { onMount, onDestroy, getContext, untrack } from 'svelte';
	import { browser } from '$app/environment';
	import cytoscape from 'cytoscape';
	// @ts-ignore
	import elk from 'cytoscape-elk';
	// @ts-ignore
	import expandCollapse from 'cytoscape-expand-collapse';
	import { toaster } from '$lib/components/toaster';
	import { hasKnowledgeProvenance } from '$lib/knowledgeProvenance';

	import { getGraphStyle, applyCompromisedStyle, getK8sCredentialIcon } from './graph_style';
	import { createElkLayout, isValidPosition, DEFAULT_LAYOUT_PARAMS } from './elk_layout';
	import type { LayoutParams } from './elk_layout';
	import GraphLayoutPlayground from './GraphLayoutPlayground.svelte';
	import { isInformational } from './edge_categories';
	import type { Node, Edge } from '$lib/api/index';
	import { getCampaignState } from '$lib/components/CampaignState.svelte';
	import GraphNodeSelector from './graph_node_selector.svelte';
	import GraphFilter from './graph_filter.svelte';
	import { workloadCompoundIds } from './workload_compounds';
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
		data: Node & { scenarioProvided: boolean };
		position?: { x: number; y: number };
	};
	type Pos = { x: number; y: number };
	type PosMap = Record<string, Pos>;
	type ExpansionSnapshot = { right: number; visibleNodeIds: string[] };

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

	cytoscape.use(elk);
	if (typeof expandCollapse === 'function') {
		cytoscape.use(expandCollapse);
	} else if (expandCollapse?.default) {
		cytoscape.use(expandCollapse.default);
	}

	// cytoscape("layout", "hierarchyFlow", hierarchyLayout);
    // cytoscape('layout', 'claude', K8sAttackGraphLayout);

	let cy: cytoscape.Core = $state() as cytoscape.Core;
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
	// Versioned because workload compounds now default to collapsed even with one pod.
	const COLLAPSED_KEY = '_collapsedNodesV2';

	let previousNodeIds: Set<string> = new Set();
	let previousWorkloadCompoundIds: Set<string> = new Set();
	let layoutParams: LayoutParams = $state({ ...DEFAULT_LAYOUT_PARAMS });
	const expansionSnapshots = new Map<string, ExpansionSnapshot>();
	let isRestoringCollapsedState = false;
	const EXPANSION_GUTTER = 32;

	function runElkLayout() {
		if (!cy || cy.nodes().length === 0) return;
		const currentPan = cy.pan();
		const currentZoom = cy.zoom();
		const l = cy.elements(':visible').layout(createElkLayout(positions, layoutParams) as any);
		l.one('layoutstop', () => {
			cy.pan(currentPan);
			cy.zoom(currentZoom);
			savePositions();
		});
		l.run();
	}

	const theme: { isDark: boolean } = getContext('theme');

	// Re-apply cytoscape styles when theme changes
	$effect(() => {
		const isDark = theme.isDark;
		if (cy) {
			const textColor = isDark ? 'white' : 'black';
			cy.nodes().style('color', textColor);
			cy.nodes("node[kind='K8sCredential']").style(
				'background-image',
				getK8sCredentialIcon(isDark)
			);
			cy.edges('[!informational]').style({
				'color': textColor,
				'line-color': textColor,
				'target-arrow-color': textColor
			});
		}
	});

	// Keep Cytoscape's visual selection in sync when another UI surface (for
	// example the operation timeline) changes the bound selected entity.
	$effect(() => {
		const id = selectedObjectId;
		if (!cy) return;

		if (!id) {
			cy.$(':selected').unselect();
			return;
		}

		const element = cy.getElementById(id);
		if (element.empty() || element.selected()) return;

		cy.batch(() => {
			element.select();
			cy.$(':selected').not(element).unselect();
		});
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
			layout: { name: 'preset' },
			zoom: zoom,
			wheelSensitivity: 0.1
		});
		if (prevPan) {
			cy.pan(prevPan);
		}

		// Initialize expand-collapse extension
		let api;
		try {
			api = (cy as any).expandCollapse({
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
			saveCollapsedNodes();
		});

		cy.on('expandcollapse.beforeexpand', (event) => {
			if (!isRestoringCollapsedState) captureExpansionSnapshot(event.target);
		});

		cy.on('expandcollapse.afterexpand', (event) => {
			handleAfterExpand(event.target);
			if (!isRestoringCollapsedState) shiftNodesForExpansion(event.target);
			saveCollapsedNodes();
		});

		// `unselect` handler must be registered first because it resets selectedNode (in case nothing is selected anymore)
		cy.on('unselect', resetSelection);
		cy.on('select', handleSelection);
		cy.on('mouseover', 'edge', (event) => event.target.addClass('hovered'));
		cy.on('mouseout', 'edge', (event) => event.target.removeClass('hovered'));

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
			sessionStorage.removeItem(ZOOM_KEY);				sessionStorage.removeItem(COLLAPSED_KEY);			// Don't clear FILTER_NS_KEY - preserve namespace filter across campaigns
			positions = {};
			previousNodeIds.clear();
			previousWorkloadCompoundIds.clear();
			// Don't reset hiddenNamespaces - user preferences should persist
		}
		previousCampaignId = currentCampaignId;
	});

	// Persist namespace filter state and re-apply it when it changes.
	$effect(() => {
		const ns = hiddenNamespaces;
		if (browser) {
			sessionStorage.setItem(FILTER_NS_KEY, JSON.stringify([...ns]));
		}
		if (cy) {
			applyNamespaceFilters(cy, ns);
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
					const currentWorkloadCompoundIds = workloadCompoundIds(graph.nodes);
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

					// Expand collapsed nodes FIRST so their children are restored into the graph
					// before we snapshot element IDs. If we snapshot before expand, the children
					// would not be in cyNodeIdSet and cy.add() would try to re-add them, causing
					// "element already exists" or "invalid ID" errors from the plugin's meta-nodes.
					const collapsedNodes: string[] = [];
					const ecApi = (window as any).cyExpandCollapseAPI;
					let addedNodeIds = new Set<string>();
					const recollapseNodes = () => {
						if (!ecApi || collapsedNodes.length === 0) return;
						new Set(collapsedNodes).forEach(id => {
							const node = cy.getElementById(id);
							if (node.length > 0 && node.isParent()) {
								// The collapse plugin restores children by applying the parent's
								// later movement delta. Seed newly added children at their parent
								// so they follow that delta instead of restoring from (0, 0).
								node.children().forEach((child: any) => {
									if (!addedNodeIds.has(child.id())) return;
									const position = node.position();
									child.position(position);
									positions[child.id()] = position;
								});
								try { ecApi.collapse(node); } catch (_) {}
							}
						});
					};
					// On initial mount, seed with IDs persisted from the previous navigation
					let hasStoredCollapseState = false;
					if (previousNodeIds.size === 0 && browser) {
						try {
							const stored = sessionStorage.getItem(COLLAPSED_KEY);
							hasStoredCollapseState = stored !== null;
							if (stored) (JSON.parse(stored) as string[]).forEach(id => collapsedNodes.push(id));
						} catch (_) {}
					}
					// Multi-pod workload compounds start collapsed. An explicitly persisted
					// expansion wins on remount; workloads that newly gain a second pod are
					// collapsed when they first become useful as a group.
					if (previousNodeIds.size === 0 && !hasStoredCollapseState) {
						currentWorkloadCompoundIds.forEach(id => collapsedNodes.push(id));
					} else if (previousNodeIds.size > 0) {
						currentWorkloadCompoundIds.forEach(id => {
							if (!previousWorkloadCompoundIds.has(id)) collapsedNodes.push(id);
						});
					}
					if (ecApi) {
						isRestoringCollapsedState = true;
						try {
							cy.nodes('.cy-expand-collapse-collapsed-node').forEach((n: any) => {
								collapsedNodes.push(n.id());
								try { ecApi.expand(n); } catch (_) {}
							});
						} finally {
							isRestoringCollapsedState = false;
						}
					}

					// Snapshot element IDs AFTER expansion so restored children are included
					const cyNodeIdSet = new Set<string>();
					cy.nodes().forEach((n: any) => { cyNodeIdSet.add(n.id()); });
					const cyEdgeIdSet = new Set<string>();
					cy.edges().forEach((e: any) => { cyEdgeIdSet.add(e.id()); });

					// Compute diffs: what to add, what to remove (guard against empty IDs)
					const newEdgeIds = new Set<string>(edges.filter((e: any) => e.data.id).map((e: any) => e.data.id as string));
					const nodesToAdd = nodes.filter((n: any) => n.data.id && !cyNodeIdSet.has(n.data.id as string));
					addedNodeIds = new Set(nodesToAdd.map((node: any) => node.data.id as string));
					const edgesToAdd = edges.filter((e: any) => e.data.id && !cyEdgeIdSet.has(e.data.id as string));

					// Remove elements no longer in the graph
					cy.nodes().filter((n: any) => n.id() && !currentNodeIds.has(n.id())).remove();
					cy.edges().filter((e: any) => e.id() && !newEdgeIds.has(e.id())).remove();

					// Update data for existing nodes (e.g. compromised/isRunning status changes)
					nodes.filter((n: any) => n.data.id && cyNodeIdSet.has(n.data.id as string)).forEach((n: any) => {
						cy.getElementById(n.data.id).data(n.data);
					});

					// Pre-position new nodes near their connected existing nodes so they don't spawn randomly
					if (nodesToAdd.length > 0) {
						const addingIds = new Set<string>(nodesToAdd.map((n: any) => n.data.id as string));
						const nodeDefinitions = new Map<string, any>(
							nodes.map((node: any) => [node.data.id as string, node])
						);
						nodesToAdd.forEach((newNode: any, index: number) => {
							if (newNode.position) return; // already has a saved position
							const nodeId = newNode.data.id as string;

							// Containment is encoded as a parent pointer rather than an edge. Start
							// new descendants at their nearest existing compound ancestor so the
							// layout animation reads as that region opening to reveal its contents.
							let ancestorId: string | undefined = newNode.data.parent;
							const visitedAncestors = new Set<string>();
							while (ancestorId && !visitedAncestors.has(ancestorId)) {
								visitedAncestors.add(ancestorId);
								const ancestor = cy.getElementById(ancestorId);
								if (ancestor.nonempty()) {
									const anchor = ancestor.position();
									// A tiny deterministic offset prevents exact overlap while keeping
									// every child visually sourced from the same compound region.
									const angle = index * 2.399963229728653;
									newNode.position = {
										x: anchor.x + Math.cos(angle) * 4,
										y: anchor.y + Math.sin(angle) * 4
									};
									break;
								}
								ancestorId = nodeDefinitions.get(ancestorId)?.data.parent;
							}
							if (newNode.position) return;

							const neighborPositions: { x: number; y: number }[] = [];
							edges.forEach((edge: any) => {
								const src = edge.data.source as string;
								const tgt = edge.data.target as string;
								const neighborId = src === nodeId ? tgt : tgt === nodeId ? src : null;
								if (neighborId && !addingIds.has(neighborId)) {
									const neighbor = cy.getElementById(neighborId);
									if (neighbor.length > 0) neighborPositions.push(neighbor.position());
								}
							});
							if (neighborPositions.length > 0) {
								const avgX = neighborPositions.reduce((s, p) => s + p.x, 0) / neighborPositions.length;
								const avgY = neighborPositions.reduce((s, p) => s + p.y, 0) / neighborPositions.length;
								// Place near neighbor centroid with a small offset to avoid exact overlap
								const angle = Math.random() * 2 * Math.PI;
								const r = 80 + Math.random() * 40;
								newNode.position = { x: avgX + Math.cos(angle) * r, y: avgY + Math.sin(angle) * r };
							}
						});
						// Ensure compound/parent nodes are added before their children
						nodesToAdd.sort((a: any, b: any) => {
							const aIsParent = nodes.some((n: any) => n.data.parent === a.data.id);
							const bIsParent = nodes.some((n: any) => n.data.parent === b.data.id);
							if (aIsParent && !bIsParent) return -1;
							if (!aIsParent && bIsParent) return 1;
							return 0;
						});
						cy.add(nodesToAdd);

						// Sync pre-computed positions into the map and onto the live element so
						// elk.position hints reflect the pre-positioned location on next layout run.
						nodesToAdd.forEach((newNode: any) => {
							if (newNode.position) {
								const id = newNode.data.id as string;
								positions[id] = newNode.position;
								// Also explicitly set the position on the live cy element (belt-and-suspenders).
								cy.getElementById(id).position(newNode.position);
							}
						});
					}

					if (edgesToAdd.length > 0) {
						cy.add(edgesToAdd);
					}
					syncWorkloadCompromisedState(cy, graph.nodes);
					// Tint pods while they are present in the live collection. The collapse
					// extension preserves their style while they are hidden inside a workload.
					applyCompromisedStyle(cy);

					recollapseNodes();

					// Only re-layout if there are new nodes or nodes were removed
					if (hasNewNodes || hasFewerNodes || previousNodeIds.size === 0) {
						console.log(`Graph changed: ${hasNewNodes ? 'new nodes' : hasFewerNodes ? 'nodes removed' : 'initial load'}`);

						const containerRect = graphContainer.getBoundingClientRect();
						if (containerRect.width === 0 || containerRect.height === 0) {
							console.warn('Graph container has zero dimensions, skipping layout');
							previousNodeIds = currentNodeIds;
							return;
						}

						const currentPan = cy.pan();
						const currentZoom = cy.zoom();
						const isInitialLoad = previousNodeIds.size === 0;

						const layoutOptions = createElkLayout(positions, untrack(() => layoutParams));
						const l = cy.elements(':visible').layout(layoutOptions as any);

						l.one('layoutstop', () => {
							if (isInitialLoad) {
								cy.fit(undefined, 50);
								if (cy.zoom() > 2) cy.zoom(2);
								cy.center();
							} else {
								if (currentPan && (currentPan.x !== 0 || currentPan.y !== 0)) {
									cy.pan(currentPan);
								}
								cy.zoom(currentZoom);
							}
							savePositions();
							console.log('ELK layout complete');
						});

						l.run();
						previousNodeIds = currentNodeIds;
					} else {
						console.log('No new nodes, skipping layout');
					}

					applyCompromisedStyle(cy);
					applyNamespaceFilters(cy, hiddenNamespaces);
					previousWorkloadCompoundIds = currentWorkloadCompoundIds;

					// Reapply text color based on current theme
					const textColor = theme.isDark ? 'white' : 'black';
					cy.nodes().style('color', textColor);
					cy.edges('[!informational]').style('color', textColor);

					// use timeout 0 to not track selectedObject as a dependency
					setTimeout(() => {
						// update the currently selected graph object
						if (selectedObject !== undefined) {
							if ('entity' in selectedObject && selectedObject.entity !== undefined) {
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

	function saveCollapsedNodes() {
		if (!browser || !cy) return;
		const ids: string[] = [];
		cy.nodes('.cy-expand-collapse-collapsed-node').forEach((n: any) => { ids.push(n.id()); });
		sessionStorage.setItem(COLLAPSED_KEY, JSON.stringify(ids));
	}

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
			const validated: PosMap = {};
			for (const [id, pos] of Object.entries(stored)) {
				if (isValidPosition(pos)) {
					validated[id] = pos;
				} else {
					console.warn(`Invalid position for node ${id}, skipping`);
				}
			}
			return validated;
		} catch (e) {
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

	function getPanPositionOrDefault(defaultValue: Pos | undefined): Pos | undefined {
		if (browser) {
			const pan = sessionStorage.getItem(PAN_KEY);
			return pan ? JSON.parse(pan) : defaultValue;
		}
		return defaultValue;
	}

	function handleSelection(event: cytoscape.EventObject) {
		let el = event.target;
		selectedObject = el.data();
		selectedObjectId = el.data()['id'];
		focusSelection(el);
		console.group("Selected Graph object");
		console.log(el.data());
		console.log(el.classes());
		console.groupEnd();
	}

	function resetSelection(event: cytoscape.EventObject) {
		// Switching selection selects the new element before unselecting the old
		// one. Do not let the old element's event clear the shared selection.
		if (cy.$(':selected').length > 0) return;
		selectedObject = undefined;
		selectedObjectId = '';
		clearSelectionFocus();
	}

	function clearSelectionFocus() {
		cy?.elements().removeClass('context-dimmed');
	}

	/**
	 * Keep the selected element and its immediate graph context prominent.
	 * Compound ancestors remain visible as quiet orientation boundaries.
	 */
	function focusSelection(element: any) {
		if (!cy) return;
		const visible = cy.elements(':visible');
		let context: cytoscape.CollectionReturnValue;

		if (element.isNode()) {
			context = element.closedNeighborhood();
			if (element.isParent()) {
				// Reveal exactly one containment level. Nested compounds remain useful
				// orientation points without also exposing their own children.
				const children = element.children();
				context = context.union(children);
				const contextNodeIds = new Set<string>();
				context.nodes().forEach((node) => { contextNodeIds.add(node.id()); });
				const internalEdges = visible.edges().filter((edge) =>
					contextNodeIds.has(edge.source().id()) && contextNodeIds.has(edge.target().id())
				);
				context = context.union(internalEdges);
			}
		} else {
			const endpoints = element.source().union(element.target());
			context = element.union(endpoints);
		}

		// Preserve compound boundaries for every prominent node without pulling
		// unrelated siblings into focus.
		context.nodes().forEach((node) => {
			context = context.union(node.ancestors());
		});

		visible.addClass('context-dimmed');
		context.removeClass('context-dimmed');
	}

	function toCyNode(n: Node, nodePos: Record<string, any>): CyNode {
		let cyNode: CyNode = {
			id: n.id,
			label: n.name,
			data: { ...n, scenarioProvided: hasKnowledgeProvenance(n.provenance, 'scenario') }
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
				scenarioProvided: hasKnowledgeProvenance(e.provenance, 'scenario'),
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
	 * Additionally, always hide "runs-on" edges when ANY other edge (informational
	 * or not) exists for that pair, since runs-on is purely structural noise.
	 * Skips edges that are already hidden by the namespace filter.
	 */
	function hideRedundantInformationalEdges(cy: cytoscape.Core) {
		// Collect directed node-pairs that have at least one non-informational, non-filtered edge
		const hasActionableEdge = new Set<string>();
		// Collect directed node-pairs that have any non-filtered edge (keyed by pair + edge name)
		const pairEdgeNames = new Map<string, Set<string>>();

		cy.edges().forEach((e: any) => {
			if (e.hasClass('namespace-filtered')) return;
			const pair = `${e.source().id()}->${e.target().id()}`;
			if (!e.data('informational')) {
				hasActionableEdge.add(pair);
			}
			// Track all edge names per directed pair
			if (!pairEdgeNames.has(pair)) pairEdgeNames.set(pair, new Set());
			pairEdgeNames.get(pair)!.add(e.data('name'));
		});

		// Hide informational edges whose directed pair has an actionable edge.
		// For "runs-on", hide when ANY other edge exists for the same pair.
		cy.edges('[?informational]').forEach((e: any) => {
			if (e.hasClass('namespace-filtered')) return; // don't touch namespace-filtered edges
			const pair = `${e.source().id()}->${e.target().id()}`;
			const name = e.data('name');

			if (name === 'runs-on') {
				// Hide runs-on if any other relation exists for this pair
				const names = pairEdgeNames.get(pair);
				if (names && names.size > 1) {
					e.hide();
				} else {
					e.show();
				}
			} else if (hasActionableEdge.has(pair)) {
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
				// Match both expanded compound nodes (isParent) and collapsed ones
				// (cy-expand-collapse-collapsed-node class). When collapsed, isParent()
				// returns false because the plugin has removed children from the graph.
				const isNamespaceNode =
					n.data('name') === nsName &&
					(n.isParent() || n.hasClass('cy-expand-collapse-collapsed-node'));
				if (isNamespaceNode) {
					filteredNodeIds.add(n.id());
					// descendants() is empty for collapsed nodes (children are removed by the
					// plugin), so this is a no-op for them — which is correct: hiding the
					// collapsed compound already hides everything inside it.
					n.descendants().forEach((d: any) => filteredNodeIds.add(d.id()));
				}
			});
		});

		// Step 3: hide filtered nodes
		filteredNodeIds.forEach((id) => {
			const n = cy.getElementById(id);
			if (n.length > 0) {
				n.addClass('namespace-filtered');
				(n as any).hide();
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
					if (edge.length > 0) (edge as any).show();
				});

				metaEdge.remove();
			}
		});

		// Re-apply informational edge filtering after expanding
		hideRedundantInformationalEdges(cy);
		applyCompromisedStyle(cy);
	}

	function captureExpansionSnapshot(node: any) {
		const bounds = node.boundingBox();
		expansionSnapshots.set(node.id(), {
			right: bounds.x2,
			visibleNodeIds: cy.nodes(':visible').map((n: any) => n.id())
		});
	}

	function shiftNodesForExpansion(node: any) {
		const snapshot = expansionSnapshots.get(node.id());
		expansionSnapshots.delete(node.id());
		if (!snapshot) return;

		const addedWidth = node.boundingBox().x2 - snapshot.right;
		if (addedWidth <= 0) return;

		const shift = addedWidth + EXPANSION_GUTTER;
		const candidates = snapshot.visibleNodeIds
			.map((id) => cy.getElementById(id))
			.filter((candidate: any) =>
				candidate.length > 0 &&
				candidate.visible() &&
				candidate.id() !== node.id() &&
				candidate.boundingBox().x1 >= snapshot.right
			);
		const candidateIds = new Set(candidates.map((candidate: any) => candidate.id()));
		const nodesToShift = candidates.filter((candidate: any) =>
			candidate.ancestors().every((ancestor: any) => !candidateIds.has(ancestor.id()))
		);

		cy.batch(() => {
			nodesToShift.forEach((candidate: any) => {
				const position = candidate.position();
				candidate.position({ x: position.x + shift, y: position.y });
			});
		});
		savePositions();
	}

	function syncWorkloadCompromisedState(cy: cytoscape.Core, graphNodes: Node[]) {
		const compromisedParents = new Set(
			graphNodes
				.filter((node) => node.kind === 'Pod' && node.compromised && node.parent)
				.map((node) => node.parent!)
		);

		workloadCompoundIds(graphNodes).forEach((id) => {
			const workload = cy.getElementById(id);
			if (workload.nonempty()) {
				workload.data('containsCompromised', compromisedParents.has(id));
			}
		});
	}

</script>

<div class={['graph-wrapper', className]}>
	<div id="graph" bind:this={graphContainer}></div>
	<GraphFilter {availableNamespaces} bind:hiddenNamespaces />
	<GraphLayoutPlayground bind:params={layoutParams} onRelayout={runElkLayout} />
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
