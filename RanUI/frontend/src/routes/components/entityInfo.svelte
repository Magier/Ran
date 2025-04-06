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
	<div><span>Name:</span> {selectedNode?.name}</div>
	<!-- {#if selectedNode?.kind} -->
	<div><span>Kind:</span> {selectedNode?.kind}</div>

	{#if selectedNode?.ip}
		<div><span>IP:</span> {selectedNode?.ip}</div>{/if}
	{#if selectedNode?.username}
		<div><span>Username:</span> {selectedNode?.username}</div>{/if}
	{#if selectedNode?.accessLevel}
		<div><span>AccessLevel:</span> {selectedNode?.accessLevel}</div>{/if}
	{#if selectedNode?.os}
		<div><span>OS:</span> {selectedNode?.os}</div>{/if}
	{#if selectedNode?.version}
		<div><span>Version:</span> {selectedNode?.version}</div>{/if}

	{#if selectedNode?.entitlements}
		{#each selectedNode.entitlements as e}
			<div><span>Can {e.verbs.join(', ')}</span>{e.resourceTypes}</div>
		{/each}
	{/if}

	<!-- <div bind:this={arrowEl} class="arrow variant-filled"></div> -->
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
