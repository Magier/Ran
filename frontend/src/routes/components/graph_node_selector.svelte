<script lang="ts">
	import { browser } from '$app/environment';
	import type cytoscape from 'cytoscape';

	type CyNode = {
		id: string;
		label: string;
		data: any;
		position?: { x: number; y: number };
	};

	type GraphNodeSelectorProps = {
		cy: cytoscape.Core;
		isOpen: boolean;
	};

	let { cy, isOpen = $bindable() }: GraphNodeSelectorProps = $props();

	let searchQuery = $state('');
	let searchResults = $state<CyNode[]>([]);
	let selectedSearchIndex = $state(0);

	// Global keydown handler for Escape key
	function handleGlobalKeydown(event: KeyboardEvent) {
		if (!isOpen) return;
		
		console.log('Global key pressed:', event.key, 'Code:', event.code, 'KeyCode:', event.keyCode);
		
		if (event.key === 'Escape' || event.code === 'Escape' || event.keyCode === 27) {
			event.preventDefault();
			event.stopPropagation();
			// Blur any focused element to prevent needing to press Escape twice
			if (document.activeElement instanceof HTMLElement) {
				document.activeElement.blur();
			}
			isOpen = false;
		}
	}

	// Add/remove global listener when dialog opens/closes
	$effect(() => {
		if (browser) {
			if (isOpen) {
				window.addEventListener('keydown', handleGlobalKeydown);
			} else {
				window.removeEventListener('keydown', handleGlobalKeydown);
			}
		}
		
		return () => {
			if (browser) {
				window.removeEventListener('keydown', handleGlobalKeydown);
			}
		};
	});

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
			isOpen = false;
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
		}
	}

	// Reactive search - update results when query changes
	$effect(() => {
		if (isOpen) {
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
		if (!isOpen && cy) {
			cy.nodes().data('highlighted', false);
		}
	});

	// Reset search state when opened
	$effect(() => {
		if (isOpen) {
			searchQuery = '';
			searchResults = [];
			selectedSearchIndex = 0;
			// Focus search input after dialog opens
			setTimeout(() => {
				document.getElementById('node-search-input')?.focus();
			}, 100);
		}
	});
</script>

{#if isOpen}
	<div class="fixed inset-0 z-50 flex items-start justify-center pt-20">
		<div 
			class="fixed inset-0 bg-black/50" 
			role="button" 
			tabindex="0"
			onclick={() => (isOpen = false)}
			onkeydown={(e) => e.key === 'Enter' && (isOpen = false)}
		></div>
		<div
			class="relative w-full max-w-lg bg-surface-200-800 border border-gray-700 rounded-lg shadow-lg p-6"
			role="dialog"
			aria-modal="true"
		>
			<h2 class="text-xl font-semibold mb-2 text-surface-contract-400">Search Nodes</h2>
			<p class="text-sm text-surface-contract-300 mb-4">
				Search for nodes by name or ID. Use arrow keys to navigate, Enter to select.
			</p>

			<input
				id="node-search-input"
				type="text"
				bind:value={searchQuery}
				onkeydown={handleSearchKeydown}
				placeholder="Type to search..."
				class="input"
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
								<div class="font-medium text-surfacecontract">{result.label}</div>
							</div>
							<div class="text-sm text-gray-400">{result.id}</div>
						</button>
					{/each}
				</div>
			{:else if searchQuery.trim() !== ''}
				<div class="mt-4 text-center text-gray-500 py-4">No nodes found</div>
			{/if}

			<div class="mt-4 text-xs text-surface-contrast-800">
				Press
				<kbd class="px-1.5 py-0.5 bg-gray-800 border border-gray-600 rounded text-gray-300">↑</kbd>
				<kbd class="px-1.5 py-0.5 bg-gray-800 border border-gray-600 rounded text-gray-300">↓</kbd>
				to navigate,
				<kbd class="px-1.5 py-0.5 bg-gray-800 border border-gray-600 rounded text-gray-300">Enter</kbd>
				to select,
				<kbd class="px-1.5 py-0.5 bg-gray-800 border border-gray-600 rounded text-gray-300">Esc</kbd>
				to close
			</div>
		</div>
	</div>
{/if}
