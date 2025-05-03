<script lang="ts">
	import { main } from '$lib/wailsjs/go/models';

	let { selectedNode } = $props();

	$inspect(selectedNode).with((type, val) => {
		console.log('inspect type: ' + type);
		console.log('selectedNode', selectedNode);
	});
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
