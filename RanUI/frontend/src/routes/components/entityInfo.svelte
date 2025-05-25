<script lang="ts">
	let { selectedNode } = $props();

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
	class="card variant-filled-secondary details-popup bg-surface-50-950 z-100 p-4 {selectedNode
		? 'show'
		: ''}"
	data-popup
>
	{#if selectedNode?.entity !== null}
		{#each Object.entries(selectedNode?.entity || {}) as [label, data]}
			{#if data !== ''}
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
