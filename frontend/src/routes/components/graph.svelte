<script lang="ts">
	import { onMount, onDestroy, getContext } from 'svelte';
	import { browser } from '$app/environment';
	import cytoscape from 'cytoscape';
	import fcose from 'cytoscape-fcose';
	import { toaster } from '$lib/components/toaster';

	import { getGraphStyle, layout, applyCompromisedStyle } from './graph_style';
	import type { Node, Edge } from '$lib/api/index';
	import { getCampaignState } from '$lib/components/CampaignState.svelte';
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
	let searchQuery = $state('');
	let searchResults = $state<CyNode[]>([]);
	let selectedSearchIndex = $state(0);

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

	const theme = getContext('theme');

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
			style: getGraphStyle(),
			layout: layout,
			zoom: zoom,
			wheelSensitivity: 0.1
		});
		if (prevPan) {
			cy.pan(prevPan);
		}

		layout.stop = () => {
			existingNodes.unlock();
			// apply previous pan and zoom, to nullify side-effects from layout
			const origPan = cy.pan();
			if (
				prevPan === undefined ||
				(origPan.x === 0 && origPan.y === 0)
			) { 
				console.error("No previous pan found, centering graph");
				cy.center();
			} else {
				console.error("Post layout pan was: ", prevPan);
			}

			cy.zoom(getZoomLevelOrDefault(2));
		};


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
		if (Object.keys(positions).length === 0) {
			console.info("No previous positions, so using sane defaults for the layout")
			cy.layout(layout).run();
			cy.center();
			saveGraphLayout();
		} else {
			console.info(">> Loaded previous node positions from session storage");
		}
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

	function toCyNode(n: Node, nodePos: Record<string, any>): CyNode {
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

		if (event.key === 'f' && !searchOpen) {
			event.preventDefault();
			openSearch();
		}
	}

	function openSearch() {
		searchOpen = true;
		searchQuery = '';
		searchResults = [];
		selectedSearchIndex = 0;
		// Focus search input after dialog opens
		setTimeout(() => {
			document.getElementById('node-search-input')?.focus();
		}, 100);
	}

	function performSearch() {
		if (!cy || searchQuery.trim() === '') {
			searchResults = [];
			return;
		}

		const query = searchQuery.toLowerCase();
		const allNodes = cy.nodes().map(n => ({
			id: n.id(),
			label: n.data('name') || n.data('label') || n.id(),
			data: n.data()
		}));

		// Filter nodes by matching query in label or id
		searchResults = allNodes.filter(n => 
			n.label.toLowerCase().includes(query) || 
			n.id.toLowerCase().includes(query)
		);
		selectedSearchIndex = 0;
	}

	function selectSearchResult(index: number) {
		if (searchResults.length === 0 || !cy) return;

		const result = searchResults[index];
		const node = cy.getElementById(result.id);
		
		if (node) {
			// Unselect all nodes first
			cy.elements().unselect();
			// Select the node
			node.select();
			// Center on the node
			cy.animate({
				center: { eles: node },
				zoom: 2,
				duration: 300
			});
			// Close search
			searchOpen = false;
		}
	}

	function handleSearchKeydown(event: KeyboardEvent) {
		if (event.key === 'ArrowDown') {
			event.preventDefault();
			selectedSearchIndex = Math.min(selectedSearchIndex + 1, searchResults.length - 1);
		} else if (event.key === 'ArrowUp') {
			event.preventDefault();
			selectedSearchIndex = Math.max(selectedSearchIndex - 1, 0);
		} else if (event.key === 'Enter') {
			event.preventDefault();
			selectSearchResult(selectedSearchIndex);
		} else if (event.key === 'Escape') {
			searchOpen = false;
		}
	}

	// Reactive search - update results when query changes
	$effect(() => {
		if (searchOpen) {
			performSearch();
		}
	});

	// Highlight the node in the graph when navigating search results
	$effect(() => {
		if (!cy || searchResults.length === 0) return;

		// Clear all highlighted nodes
		cy.nodes().data('highlighted', false);

		// Highlight the current search result node
		if (selectedSearchIndex >= 0 && selectedSearchIndex < searchResults.length) {
			const result = searchResults[selectedSearchIndex];
			const node = cy.getElementById(result.id);
			if (node) {
				node.data('highlighted', true);
			}
		}
	});

	// Clear highlighted state when search closes
	$effect(() => {
		if (!searchOpen && cy) {
			cy.nodes().data('highlighted', false);
		}
	});
</script>

<div id="graph" class={['bg-tertiary-surface-800-200', className]} bind:this={graphContainer}></div>

{#if searchOpen}
	<div class="fixed inset-0 z-50 flex items-start justify-center pt-20">
		<div 
			class="fixed inset-0 bg-black/50" 
			role="button" 
			tabindex="0"
			onclick={() => (searchOpen = false)}
			onkeydown={(e) => e.key === 'Enter' && (searchOpen = false)}
		></div>
		<div
			class="relative w-full max-w-lg bg-gray-900 border border-gray-700 rounded-lg shadow-lg p-6"
		>
			<h2 class="text-xl font-semibold mb-2 text-white">Search Nodes</h2>
			<p class="text-sm text-gray-400 mb-4">
				Search for nodes by name or ID. Use arrow keys to navigate, Enter to select.
			</p>

			<input
				id="node-search-input"
				type="text"
				bind:value={searchQuery}
				onkeydown={handleSearchKeydown}
				placeholder="Type to search..."
				class="w-full px-4 py-2 bg-gray-800 border border-gray-600 rounded-md text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-blue-500"
			/>

			{#if searchResults.length > 0}
				<div class="mt-4 max-h-64 overflow-y-auto border border-gray-700 rounded-md">
					{#each searchResults as result, index}
						<button
							type="button"
							class="w-full px-4 py-2 text-left hover:bg-gray-700 transition-colors {index ===
							selectedSearchIndex
								? 'bg-gray-700'
								: 'bg-gray-800'}"
							onclick={() => selectSearchResult(index)}
							onmouseenter={() => (selectedSearchIndex = index)}
						>
							<div class="flex items-center justify-between">
								<div class="font-medium text-white">{result.label}</div>
								{#if index === selectedSearchIndex}
									<span class="text-xs px-2 py-0.5 bg-blue-600 text-white rounded">highlighted</span>
								{/if}
							</div>
							<div class="text-sm text-gray-400">{result.id}</div>
						</button>
					{/each}
				</div>
			{:else if searchQuery.trim() !== ''}
				<div class="mt-4 text-center text-gray-500 py-4">No nodes found</div>
			{/if}

			<div class="mt-4 text-xs text-gray-500">
				Press
				<kbd class="px-1.5 py-0.5 bg-gray-800 border border-gray-600 rounded text-gray-300"
					>↑</kbd
				>
				<kbd class="px-1.5 py-0.5 bg-gray-800 border border-gray-600 rounded text-gray-300"
					>↓</kbd
				>
				to navigate,
				<kbd class="px-1.5 py-0.5 bg-gray-800 border border-gray-600 rounded text-gray-300"
					>Enter</kbd
				>
				to select,
				<kbd class="px-1.5 py-0.5 bg-gray-800 border border-gray-600 rounded text-gray-300"
					>Esc</kbd
				>
				to close
			</div>
		</div>
	</div>
{/if}

<style>
	#graph {
		width: 100%;
		/* height: 1000px; */
		display: block;
		/* background-color: #1a1a1a; */
	}
</style>
