<script lang="ts">
	import Tree from '$lib/components/tree.svelte';
	import EntitlementInfo from './entitlement_info.svelte';
	import { domain } from '$lib/wailsjs/go/models';
	import { showToast } from '$lib/components/toaster';
	import { getCampaignState } from '$lib/components/CampaignState.svelte';


	type EntitlementInfoProps = {
		selectedObject: any; //TODO: define a proper type
		sendAction?: (ttp: domain.TTP, args: any) => void;
	};

	let { selectedObject, sendAction } = $props();

	const campaignState = getCampaignState();
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

	function readFile(path: string) {
		const ttp = campaignState.getTtpById('read-file')
		if (ttp) {
			sendAction(ttp, {"PATH":path});
		} else {
			showToast("TTP 'read-file' not found", '', 'error');
		}
	}
</script>

<!-- specify data-popup attr. for consistent styling via skeleton-ui -->
<!-- class="card variant-filled-secondary details-popup bg-surface-50-950 z-100 flex w-96 flex-col overflow-auto p-4 {selectedNode  -->
<div class="max-h-120 overflow-auto" data-popup>
	{#if selectedObject?.entity}
		{#each Object.entries(selectedObject?.entity || {}) as [label, data]}
			{#if label === 'volumeMounts' || label === 'mounts'}
				<details>
					<summary>
						<span class="font-bold mr-1">{label}</span>
						<span class="badge preset-outlined-surface-500"
							>({Array.isArray(data) ? data.length : Object.keys(data).length} items)</span
						>
					</summary>
					<Tree entries={Array.isArray(data) ? data : []} onLeafClick={readFile} />
				</details>
			{:else if label === 'can'}
				{#if Object.keys(data).length > 0}
					<details>
						<summary>
							<span class="font-bold mr-1">{label}</span>
							<span class="badge preset-outlined-surface-500"
								>({Array.isArray(data) ? data.length : Object.keys(data).length} items)</span
							>
						</summary>
						<EntitlementInfo entitlements={data as domain.RBACPermission[]} />
					</details>
				{:else}
					<div>
					<span class="font-bold mr-1">{label}</span>
					?
					</div>
					<!-- <button class="btn btn-sm preset-filled-primary-500" disabled>🔍</button> -->
				{/if}
			{:else if Array.isArray(data) && data.length > 0}
				<details>
					<summary>
						<span class="font-bold mr-1">{label}</span>
						<span class="badge preset-outlined-surface-500">({data.length} items)</span>
					</summary>
					<ul class="list-inside list-none pl-5">
						{#each data as item}
							<li>{prettyPrint(item)}</li>
						{/each}
					</ul>
				</details>
			{:else if typeof data === 'object' && data !== null}
				<!-- Collapsible section for objects/arrays -->
				<details>
					<summary>
						<span class="font-bold mr-1">{label}</span>
						<span class="badge preset-outlined-surface-500"
							>({Array.isArray(data) ? data.length : Object.keys(data).length} items)</span
						>
					</summary>
					<pre class="max-h-80 overflow-scroll">{JSON.stringify(data, null, 2)}</pre>
				</details>
			{:else if data !== ''}
				<div><span class="font-bold mr-1">{label}:</span>{prettyPrint(data)}</div>
			{/if}
		{/each}
	{:else if selectedObject?.relation !== null}
		{#each Object.entries(selectedObject?.relation || {}) as [label, data]}
			<div><span>{label}:</span> {prettyPrint(data)}</div>
		{/each}
	{:else}
		<h3>Unknown Object type</h3>
		{prettyPrint(selectedObject)}
	{/if}

	{#if selectedObject?.accessLevel}
		<div><span>AccessLevel:</span> {selectedObject?.accessLevel}</div>{/if}
	{#if selectedObject?.entitlements}
		<h4>Entitlements</h4>
		{#each selectedObject.entitlements as e}
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
