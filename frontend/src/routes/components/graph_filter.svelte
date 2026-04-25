<script lang="ts">
	import Icon from '@iconify/svelte';

	type GraphFilterProps = {
		availableNamespaces: string[];
		hiddenNamespaces: Set<string>;
		collapseWorkloads: boolean;
	};

	let { availableNamespaces, hiddenNamespaces = $bindable(), collapseWorkloads = $bindable() }: GraphFilterProps = $props();

	let panelOpen = $state(false);
	let customInput = $state('');

	const activeFilterCount = $derived(hiddenNamespaces.size + (collapseWorkloads ? 1 : 0));

	// Custom filters: those not in the detected namespace list
	const customFilters = $derived([...hiddenNamespaces].filter((ns) => !availableNamespaces.includes(ns)));

	function toggleNamespace(ns: string) {
		const next = new Set(hiddenNamespaces);
		if (next.has(ns)) {
			next.delete(ns);
		} else {
			next.add(ns);
		}
		hiddenNamespaces = next;
	}

	// function addCustomFilter() {
	// 	const ns = customInput.trim();
	// 	if (!ns) return;
	// 	hiddenNamespaces = new Set([...hiddenNamespaces, ns]);
	// 	customInput = '';
	// }

	// function handleCustomInputKeydown(e: KeyboardEvent) {
	// 	if (e.key === 'Enter') addCustomFilter();
	// }

	function removeFilter(ns: string) {
		const next = new Set(hiddenNamespaces);
		next.delete(ns);
		hiddenNamespaces = next;
	}
</script>

<!-- Backdrop to close panel -->
{#if panelOpen}
	<div
		class="fixed inset-0 z-40"
		role="presentation"
		onclick={() => (panelOpen = false)}
	></div>
{/if}

<div class="absolute bottom-1 right-3 z-50  text-surface-700-300">
	<!-- Filter toggle button -->
	<button
		class="chip preset-outlined-surface-100-900 border-surface-400-600"
		onclick={() => (panelOpen = !panelOpen)}
		title="Filter graph nodes"
		aria-label="Toggle namespace filter"
	>
		<!-- Funnel icon -->
		<Icon icon="mdi:funnel" class="inline-block text-surface-400-600" />
	</button>

	<!-- Filter panel -->
	{#if panelOpen}
		<div
			class="absolute bottom-full right-0 mb-1 w-64 bg-surface-50-950 border border-gray-200 rounded-lg shadow-xl p-4 z-50"
			role="dialog"
			aria-label="Namespace filter options"
		>
			<h3 class="text-sm font-semibold text-surface-700-300 mb-3">Hide Namespaces</h3>

			{#if availableNamespaces.length > 0}
				<div class="space-y-1.5 mb-3">
					{#each availableNamespaces as ns}
						<label class="flex items-center gap-2 cursor-pointer group">
							<input
								type="checkbox"
								checked={hiddenNamespaces.has(ns)}
								onchange={() => toggleNamespace(ns)}
								class="w-3.5 h-3.5 rounded checkbox cursor-pointer"
							/>
							<span
								class="text-sm {hiddenNamespaces.has(ns)
									? 'text-surface-400-600 line-through'
									: 'text-surface-700-300'} group-hover:text-surface-200-800 transition-colors"
							>
								{ns}
							</span>
						</label>
					{/each}
				</div>
			{:else}
				<p class="text-xs text-surface-400-600 mb-3">No namespaces detected in graph.</p>
			{/if}

			<div class="border-t border-surface-300-700 pt-3 mb-1">
				<h3 class="text-sm font-semibold text-surface-700-300 mb-2">Simplify</h3>
				<label class="flex items-center gap-2 cursor-pointer group">
					<input
						type="checkbox"
						checked={collapseWorkloads}
						onchange={() => (collapseWorkloads = !collapseWorkloads)}
						class="w-3.5 h-3.5 rounded checkbox cursor-pointer"
					/>
					<span
						class="text-sm {collapseWorkloads
							? 'text-surface-400-600'
							: 'text-surface-700-300'} group-hover:text-surface-200-800 transition-colors"
					>
						Hide single-pod workloads
					</span>
				</label>
			</div>

			<!-- Custom filters -->
			<!-- {#if customFilters.length > 0}
				<div class="mb-3">
					<p class="text-xs  mb-1.5">Custom filters</p>
					<div class="space-y-1">
						{#each customFilters as ns}
							<div class="flex items-center justify-between gap-2">
								<span class="text-sm text-surface-400-600 line-through truncate">{ns}</span>
								<button
									class="text-surface-400-600 hover:text-red-400 transition-colors flex-shrink-0 cursor-pointer"
									onclick={() => removeFilter(ns)}
									title="Remove filter"
									aria-label="Remove filter for {ns}"
								>
									<svg
										xmlns="http://www.w3.org/2000/svg"
										width="12"
										height="12"
										viewBox="0 0 24 24"
										fill="none"
										stroke="currentColor"
										stroke-width="2.5"
										stroke-linecap="round"
									>
										<line x1="18" y1="6" x2="6" y2="18" />
										<line x1="6" y1="6" x2="18" y2="18" />
									</svg>
								</button>
							</div>
						{/each}
					</div>
				</div>
			{/if} -->

			<!-- Add custom namespace -->
			<!-- <div class="border-t border-gray-700 pt-3">
				<p class="text-xs text-gray-500 mb-1.5">Add custom filter</p>
				<div class="flex gap-1.5">
					<input
						type="text"
						bind:value={customInput}
						onkeydown={handleCustomInputKeydown}
						placeholder="namespace name…"
						class="flex-1 min-w-0 px-2 py-1 bg-gray-800 border border-gray-600 rounded text-xs text-white placeholder-gray-600 focus:outline-none focus:ring-1 focus:ring-blue-500"
					/>
					<button
						onclick={addCustomFilter}
						class="px-2 py-1 bg-blue-600 hover:bg-blue-500 text-white text-xs rounded transition-colors cursor-pointer"
					>
						Add
					</button>
				</div>
			</div> -->

			{#if activeFilterCount > 0}
				<button
					onclick={() => { hiddenNamespaces = new Set(); collapseWorkloads = false; }}
					class="mt-3 w-full text-xs text-surface-400-600 hover:text-red-400 transition-colors cursor-pointer text-left"
				>
					Clear all filters
				</button>
			{/if}
		</div>
	{/if}
</div>
