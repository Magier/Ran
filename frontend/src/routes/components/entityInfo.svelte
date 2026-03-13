<script lang="ts">
	import Tree from '$lib/components/tree.svelte';
	import EntitlementInfo from './entitlement_info.svelte';
	import type {RBACPermission, TTP} from '$lib/api/index';
	import { showToast } from '$lib/components/toaster';
	import { getCampaignState } from '$lib/components/CampaignState.svelte';


	type ObjectInfoProps = {
		objectId: string;
		sendAction?: (ttp: TTP, args: any) => void;
		class: string | undefined;
	};

	let { objectId, sendAction, class: className } : ObjectInfoProps = $props();

	const campaignState = getCampaignState();
	const items = [];
	// const tree = new Tree({ items });
	const obj = $derived(campaignState.getObjectById(objectId));
	
	// Track previous values and highlighted fields
	let previousObjectId: string | null = null;
	let previousValues: Record<string, any> = {};
	let highlightedFields = $state<Record<string, boolean>>({});
	let timeouts: Map<string, number> = new Map();

	// Track changes in object fields
	$effect(() => {
		if (!obj) return;
		
		// Check if the objectId changed (user selected a different entity)
		if (previousObjectId !== objectId) {
			// Different entity selected - reset without highlighting
			previousObjectId = objectId;
			previousValues = {};
			for (const [key, value] of Object.entries(obj)) {
				previousValues[key] = value;
			}
			// Clear all highlights
			highlightedFields = {};
			timeouts.forEach(timeout => clearTimeout(timeout));
			timeouts.clear();
			return;
		}
		
		// Same entity - check each field for changes
		for (const [key, value] of Object.entries(obj)) {
			const isNewField = !(key in previousValues);
			let shouldHighlight = false;
			
			if (isNewField) {
				// New field added - highlight it
				shouldHighlight = true;
			} else {
				// Check if existing field value changed
				try {
					const prevStr = JSON.stringify(previousValues[key]);
					const currStr = JSON.stringify(value);
					
					if (prevStr !== currStr) {
						shouldHighlight = true;
					}
				} catch (e) {
					// Ignore JSON.stringify errors (e.g., circular references)
					console.warn('Error comparing values for key:', key, e);
				}
			}
			
			if (shouldHighlight) {
				// Clear any existing timeout for this field
				const existingTimeout = timeouts.get(key);
				if (existingTimeout) {
					clearTimeout(existingTimeout);
				}
				
				// Highlight the field
				highlightedFields[key] = true;
				
				// Remove highlight after animation completes (2 seconds)
				const timeoutId = setTimeout(() => {
					highlightedFields[key] = false;
					timeouts.delete(key);
				}, 2000) as unknown as number;
				
				timeouts.set(key, timeoutId);
			}
			
			// Update previous value
			previousValues[key] = value;
		}
		
		// Cleanup function - clear all timeouts when effect re-runs or component unmounts
		return () => {
			timeouts.forEach(timeout => clearTimeout(timeout));
			timeouts.clear();
		};
	});

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
		} else if (obj?.hasOwnProperty('IP')) {
			// Handle special case for objects with 'IP' property
			return obj.IP;
		} else {
			return JSON.stringify(obj, null, 2);
		}
	}

	function readFile(path: string) {
		const ttp = campaignState.getTtpById('read-file')
		if (ttp) {
			if (sendAction) {
				sendAction(ttp, {"PATH":path});
			} else {
				showToast("No sendAction function provided", '', 'error');
			}
		} else {
			showToast("TTP 'read-file' not found", '', 'error');
		}
	}
</script>

<!-- specify data-popup attr. for consistent styling via skeleton-ui -->
<!-- class="card variant-filled-secondary details-popup bg-surface-50-950 z-100 flex w-96 flex-col overflow-auto p-4 {selectedNode  -->
<div class="{className} pointer-events-auto z-[100] max-h-120 overflow-auto border border-surface-600 w-110 rounded-lg bg-surface-100-900 p-4 shadow-xl" >
	{#if obj}
		{#each Object.entries(obj || {}) as [label, data]}
			{#if label === 'containers' && Array.isArray(data) && data.length > 0}
				<!-- Special drill-down view for containers -->
				<details open class:field-changed={highlightedFields[label]}>
					<summary>
						<span class="font-bold mr-1">{label}</span>
						<span class="badge preset-outlined-surface-500">({data.length} items)</span>
					</summary>
					<div class="pl-4 space-y-4">
						{#each data as container, idx}
							<div class="border-l-2 border-surface-500 pl-3 py-2">
								<!-- Top: name and command -->
								<div class="font-bold text-primary-500">{container.name}</div>
								{#if container.command && container.command.length > 0}
									<div class="mt-1">
										<span class="text-surface-600 dark:text-surface-400 text-sm">Command:</span>
										<code class="text-xs ml-1">{container.command.join(' ')}</code>
									</div>
								{/if}
								{#if container.args && container.args.length > 0}
									<div class="mt-1">
										<span class="text-surface-600 dark:text-surface-400 text-sm">Args:</span>
										<code class="text-xs ml-1">{container.args.join(' ')}</code>
									</div>
								{/if}

								<!-- Second level: volumeMounts and ports -->
								{#if container.volumeMounts && container.volumeMounts.length > 0}
									<details class="mt-2">
										<summary class="text-sm text-surface-600 dark:text-surface-400 cursor-pointer">
											Volume Mounts ({container.volumeMounts.length})
										</summary>
										<ul class="list-inside list-disc pl-4 text-sm mt-1">
											{#each container.volumeMounts as vm}
												<li>
													<span class="font-mono">{vm.name}</span> →
													<span class="font-mono">{vm.mountPath}</span>
													{#if vm.readOnly}<span class="text-xs text-warning-500">(ro)</span>{/if}
													{#if vm.subPath}<span class="text-xs text-surface-500">[{vm.subPath}]</span>{/if}
												</li>
											{/each}
										</ul>
									</details>
								{/if}

								{#if container.ports && container.ports.length > 0}
									<details class="mt-2">
										<summary class="text-sm text-surface-600 dark:text-surface-400 cursor-pointer">
											Ports ({container.ports.length})
										</summary>
										<ul class="list-inside list-disc pl-4 text-sm mt-1">
											{#each container.ports as port}
												<li>
													{#if port.name}<span class="font-mono">{port.name}:</span> {/if}
													<span class="font-mono">{port.containerPort}/{port.protocol || 'TCP'}</span>
													{#if port.hostPort} → <span class="font-mono">{port.hostPort}</span>{/if}
												</li>
											{/each}
										</ul>
									</details>
								{/if}

								<!-- Rest of properties -->
								{#if container.image}
									<div class="mt-2 text-sm">
										<span class="text-surface-600 dark:text-surface-400">Image:</span>
										<span class="font-mono text-xs ml-1">{container.image}</span>
									</div>
								{/if}

								{#if container.env && container.env.length > 0}
									<details class="mt-2">
										<summary class="text-sm text-surface-600 dark:text-surface-400 cursor-pointer">
											Environment ({container.env.length})
										</summary>
										<ul class="list-inside list-none pl-4 text-xs mt-1 font-mono">
											{#each container.env as env}
												<li>
													{env.name}={env.value || JSON.stringify(env.valueFrom)}
												</li>
											{/each}
										</ul>
									</details>
								{/if}

								{#if container.securityContext}
									<details class="mt-2">
										<summary class="text-sm text-surface-600 dark:text-surface-400 cursor-pointer">
											Security Context
										</summary>
										<pre class="text-xs mt-1 pl-4 overflow-auto">{JSON.stringify(container.securityContext, null, 2)}</pre>
									</details>
								{/if}

								{#if container.resources}
									<details class="mt-2">
										<summary class="text-sm text-surface-600 dark:text-surface-400 cursor-pointer">
											Resources
										</summary>
										<pre class="text-xs mt-1 pl-4 overflow-auto">{JSON.stringify(container.resources, null, 2)}</pre>
									</details>
								{/if}
							</div>
						{/each}
					</div>
				</details>
			{:else if label === 'volumeMounts' || label === 'mounts'}
				<details class:field-changed={highlightedFields[label]}>
					<summary>
						<span class="font-bold mr-1">{label}</span>
						<span class="badge preset-outlined-surface-500"
							>({Array.isArray(data) ? data.length : (typeof data === 'object' && data !== null ? Object.keys(data).length : 0)} items)</span
						>
					</summary>
					<Tree entries={Array.isArray(data) ? data : []} onLeafClick={readFile} />
				</details>
			{:else if label === 'can'}
				{#if typeof data === 'object' && data !== null && Object.keys(data).length > 0}
					<details class:field-changed={highlightedFields[label]}>
						<summary>
							<span class="font-bold mr-1">{label}</span>
							<span class="badge preset-outlined-surface-500"
								>({Array.isArray(data) ? data.length : Object.keys(data).length} items)</span
							>
						</summary>
						<EntitlementInfo entitlements={data as RBACPermission[]} />
					</details>
				{:else}
					<div class:field-changed={highlightedFields[label]}>
					<span class="font-bold mr-1">{label}</span>
					?
					</div>
					<!-- <button class="btn btn-sm preset-filled-primary-500" disabled>🔍</button> -->
				{/if}
			{:else if Array.isArray(data) && data.length > 0}
				{#if data.length === 1}
					<div class:field-changed={highlightedFields[label]}><span class="font-bold mr-1">{label}:</span>{prettyPrint(data[0])}</div>
				{:else}
				<details class:field-changed={highlightedFields[label]}>
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
				{/if }
			{:else if typeof data === 'object' && data !== null}
				<!-- Collapsible section for objects/arrays -->
				<details class:field-changed={highlightedFields[label]}>
					<summary>
						<span class="font-bold mr-1">{label}</span>
						<span class="badge preset-outlined-surface-500"
							>({Array.isArray(data) ? data.length : Object.keys(data).length} items)</span
						>
					</summary>
					<pre class="max-h-80 overflow-scroll">{JSON.stringify(data, null, 2)}</pre>
				</details>
			{:else if data !== ''}
				<div class:field-changed={highlightedFields[label]}><span class="font-bold mr-1">{label}:</span>{prettyPrint(data)}</div>
			{/if}
		{/each}
	{:else}
		<h3>Unknown Object type</h3>
		<div>
			<span class="font-bold mr-1">ID</span>
			{objectId}
		</div>
		{prettyPrint(obj)}
	{/if}

	{#if obj?.accessLevel}
		<div><span>AccessLevel:</span> {obj?.accessLevel}</div>{/if}
	{#if obj?.entitlements}
		<h4>Entitlements</h4>
		{#each obj?.entitlements as e}
			<div><span>Can {e.verbs.join(', ')}</span>{e.resourceTypes}</div>
		{/each}
	{/if}
</div>

<style>
	@keyframes highlight-fade {
		0% {
			background-color: var(--color-primary-500);
			transform: scale(1.02);
		}
		100% {
			background-color: transparent;
			transform: scale(1);
		}
	}

	.field-changed {
		animation: highlight-fade 2s ease-out;
	}
</style>
