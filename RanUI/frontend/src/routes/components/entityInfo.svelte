<script lang="ts">
	import Tree from '$lib/components/tree.svelte';

	let { selectedNode } = $props();

	const items = [];
	// const tree = new Tree({ items });

	function prettyPrint(obj: any): string {
		if (typeof obj === 'string') {
			return obj;
			// } else if (typeof obj === 'object') {
			// 	if (Array.isArray(obj)) {
			// 		return obj.map((item) => prettyPrint(item)).join(', ');
			// 	} else if (obj === null) {
			// 		return 'null';
			// 	} else {
			// 		return JSON.stringify(obj, null, 2);
			// 	}
			// } else if (typeof obj === 'number') {
			// 	return obj.toString();
			// } else if (typeof obj === 'boolean') {
			// 	return obj ? 'true' : 'false';
		} else {
			return JSON.stringify(obj, null, 2);
		}
	}
</script>

<!-- specify data-popup attr. for consistent styling via skeleton-ui -->
<div
	class="card variant-filled-secondary details-popup bg-surface-50-950 z-100 flex w-96 flex-col overflow-auto p-4 {selectedNode
		? 'show'
		: ''}"
	data-popup
>
	{#if selectedNode?.entity !== null}
		{#each Object.entries(selectedNode?.entity || {}) as [label, data]}
			{#if label === 'volumeMounts'}
				<details>
					<summary>
						{label}
						<span class="badge preset-outlined-surface-500"
							>({Array.isArray(data) ? data.length : Object.keys(data).length} items)</span
						>
					</summary>
					<Tree entries={Array.isArray(data) ? data : []} />
				</details>
			{:else if typeof data === 'object' && data !== null}
				<!-- Collapsible section for objects/arrays -->
				<details>
					<summary>
						{label}
						<span class="badge preset-outlined-surface-500"
							>({Array.isArray(data) ? data.length : Object.keys(data).length} items)</span
						>
					</summary>
					<pre class="max-h-80">{JSON.stringify(data, null, 2)}</pre>
				</details>
			{:else if Array.isArray(data) && data.length > 0}
				<!-- Collapsible section for arrays -->
				<details>
					<summary>
						{label}
						<span class="badge preset-outlined-surface-500">({data.length} items)</span>
					</summary>
					<pre>{JSON.stringify(data, null, 2)}</pre>
				</details>
			{:else if data !== ''}
				<div><span>{label}:</span> {prettyPrint(data)}</div>
			{/if}
		{/each}
	{/if}

	{#if selectedNode?.accessLevel}
		<div><span>AccessLevel:</span> {selectedNode?.accessLevel}</div>{/if}
	{#if selectedNode?.entitlements}
		<h4>Entitlements</h4>
		{#each selectedNode.entitlements as e}
			<div><span>Can {e.verbs.join(', ')}</span>{e.resourceTypes}</div>
		{/each}
	{/if}
</div>

<style>
	.details-popup {
		position: absolute;
		display: none;
	}
	.details-popup.show {
		display: block;
	}
</style>
