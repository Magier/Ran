<script lang="ts">
	import Tree from '$lib/components/tree.svelte';
	import EntitlementInfo from './entitlement_info.svelte';
	import Icon from '@iconify/svelte';
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

	const EFFECT_FIELD_MAP: Record<string, string[]> = {
		'linux.mounts': ['volume_mounts'],
		'sys.envVar':   ['env_vars', 'envVars'],
		'sys.ip':       ['ips'],
		'sys.files':    ['files', 'binaries'],
		'sys.userID':   ['user_id'],
	};

	function isEmpty(data: any): boolean {
		if (data === undefined || data === null || data === '') return true;
		if (Array.isArray(data)) return data.length === 0;
		if (typeof data === 'object') return Object.keys(data).length === 0;
		return false;
	}

	let applicableTtps = $state<TTP[]>([]);

	$effect(() => {
		const id = objectId;
		if (!id) { applicableTtps = []; return; }
		campaignState.api.GetApplicableTTPs(id)
			.then((ttps) => { applicableTtps = ttps; })
			.catch(() => { applicableTtps = []; });
	});

	const fieldTtpIndex = $derived.by(() => {
		const idx = new Map<string, TTP>();
		for (const ttp of applicableTtps) {
			for (const effect of ttp.effects ?? []) {
				for (const field of EFFECT_FIELD_MAP[effect] ?? []) {
					if (!idx.has(field)) idx.set(field, ttp);
				}
			}
		}
		return idx;
	});
	
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

	// Fields handled explicitly in the header — skip from the generic loop
	const HEADER_FIELDS = new Set(['id', 'name', 'namespace', 'kind', 'entityId', 'parent', 'entity', 'compromised']);

	function shouldShowField(label: string, data: any): boolean {
		if (HEADER_FIELDS.has(label)) return false;
		if (data === undefined) return false;
		// Hide running state when positive — it's the default and duplicates phase
		if ((label === 'isRunning' || label === 'is_running') && data !== false) return false;
		// Hide phase: Running — same info as is_running: true
		if (label === 'phase' && data === 'Running') return false;
		// Hide empty owner_references
		if (label === 'owner_references' && Array.isArray(data) && data.length === 0) return false;
		// Empty field: only show if a TTP can discover it (the button is the point)
		if (isEmpty(data)) return fieldTtpIndex.has(label);
		return true;
	}

	let idCopied = $state(false);
	let tokenCopied = $state(false);

	function copyId() {
		if (!obj) return;
		navigator.clipboard.writeText(obj.id).then(() => {
			idCopied = true;
			setTimeout(() => { idCopied = false; }, 1500);
		});
	}

	function copyToken() {
		if (!obj || !obj.token || !obj.token.Raw) return;
		navigator.clipboard.writeText(obj.token.Raw).then(() => {
			tokenCopied = true;
			setTimeout(() => { tokenCopied = false; }, 1500);
		});
	}

	function ttpForField(label: string): TTP | undefined {
		return fieldTtpIndex.get(label);
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
<div class="{className} pointer-events-auto overflow-auto border border-surface-600 rounded-lg bg-surface-100-900 p-4 shadow-xl text-xs md:text-sm w-full" >
	{#if obj}
		{#snippet runBtn(label: string)}
			{@const ttp = ttpForField(label)}
			{#if ttp && sendAction}
				<button
					class="shrink-0 cursor-pointer rounded p-0.5 hover:bg-surface-300 dark:hover:bg-surface-700 transition-colors"
					title="Run: {ttp.name}"
					onclick={() => sendAction!(ttp!, {})}
				>
					<Icon icon="mdi:play-circle-outline" width="14" class="text-primary-500" />
				</button>
			{/if}
		{/snippet}
		<!-- Header: name + kind badge + copy-ID button -->
		<div class="flex items-center gap-2 mb-1">
			<span class="font-bold truncate text-sm md:text-base" class:field-changed={highlightedFields['name']}>{obj.name}{#if obj.meta?.name_confidence === 'derived'}<sup class="text-surface-400 dark:text-surface-600 cursor-help" title="Name is derived — inferred from heuristics or indirect sources, not confirmed by the Kubernetes API">*</sup>{/if}</span>
			{#if obj.kind}
				<span class="badge bg-indigo-200 text-indigo-800 text-xs shrink-0">{obj.kind}</span>
			{/if}
			<button
				class="shrink-0 cursor-pointer rounded p-0.5 hover:bg-surface-300 dark:hover:bg-surface-700 transition-colors"
				title={obj.id}
				onclick={copyId}
			>
				{#if idCopied}
					<Icon icon="mdi:check" width="16" class="text-success-500" />
				{:else}
					<Icon icon="mdi:content-copy" width="16" class="text-surface-500" />
				{/if}
			</button>
		</div>
		{#if obj.namespace}
			<div class="mb-1" class:field-changed={highlightedFields['namespace']}>
				<span class="font-semibold mr-1">Namespace:</span>{obj.namespace}
			</div>
		{/if}

		{#each Object.entries(obj || {}).filter(([label, data]) => shouldShowField(label, data)).sort(([a], [b]) => a.localeCompare(b)) as [label, data]}
			{#if label === 'containers' && Array.isArray(data) && data.length > 0}
				<!-- Special drill-down view for containers -->
				<details class="mb-1" class:field-changed={highlightedFields[label]}>
					<summary>
						<span class="font-bold">{label}</span>
						<span class="text-xs text-surface-500">({data.length})</span>
					</summary>
					<div class="pl-4 space-y-4">
						{#each data as container, idx}
							<div class="border-l-2 border-surface-500 pl-3 py-2">
								<!-- Top: name and command -->
								 <span>Name: </span>
								<div class="font-bold">{container.name}</div>
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
								{#if container.volume_mounts && container.volume_mounts.length > 0}
									<details class="mt-2">
										<summary class="text-sm text-surface-600 dark:text-surface-400 cursor-pointer">
											Volume Mounts ({container.volume_mounts.length})
										</summary>
										<ul class="list-inside list-none pl-4 space-y-0.5 mt-1">
											{#each container.volume_mounts as vm}
												<li class="flex items-center gap-1 flex-wrap text-xs">
													<span class="font-mono">{vm.mount_point}</span>
													<span class="text-surface-400">({vm.name})</span>
													{#if vm.read_only}<span class="badge bg-warning-100 text-warning-800 text-xs">ro</span>{/if}
													{#if vm.is_host_path}<span class="badge bg-error-100 text-error-800 text-xs">hostPath: {vm.mount_root}</span>{/if}
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
			{:else if (label === 'volume_mounts' || label === 'volumeMounts' || label === 'mounts') && Array.isArray(data) && data.length > 0}
				<details class="mb-1" class:field-changed={highlightedFields[label]}>
					<summary>
						<span class="font-bold">Volume Mounts</span>
						<span class="text-xs text-surface-500">({data.length})</span>
					</summary>
					<ul class="list-inside list-none pl-4 space-y-1 mt-1">
						{#each data as m}
							<li class="flex items-center gap-1 flex-wrap">
								<span class="font-mono text-xs">{m.mount_point ?? m.mountPath}</span>
								{#if m.name}
									<span class="text-surface-400 text-xs">({m.name})</span>
								{/if}
								{#if m.read_only || m.readOnly}
									<span class="badge bg-warning-100 text-warning-800 text-xs">ro</span>
								{/if}
								{#if m.is_host_path}
									<span class="badge bg-error-100 text-error-800 text-xs">hostPath: {m.mount_root}</span>
								{/if}
							</li>
						{/each}
					</ul>
				</details>
			{:else if label === 'can'}
				{#if typeof data === 'object' && data !== null && Object.keys(data).length > 0}
					<details class="mb-1" class:field-changed={highlightedFields[label]}>
						<summary>
							<span class="font-bold">{label}</span>
							<span class="text-xs text-surface-500">({Array.isArray(data) ? data.length : Object.keys(data).length})</span>
						</summary>
					<EntitlementInfo entitlements={data as RBACPermission[]} />
					</details>
				{:else}
					<div class="mb-1" class:field-changed={highlightedFields[label]}>
					<span class="font-bold mr-1">{label}</span>
					?
					</div>
					<!-- <button class="btn btn-sm preset-filled-primary-500" disabled>🔍</button> -->
				{/if}
			{:else if label === 'files' && Array.isArray(data) && data.length > 0}
				<details class="mb-1" class:field-changed={highlightedFields[label]}>
					<summary>
						<span class="font-bold">{label}</span>
						<span class="text-xs text-surface-500">({data.length})</span>
					</summary>
					<ul class="list-inside list-none pl-5">
						{#each data as item}
							<li>
								<button class="text-left hover:underline cursor-pointer" onclick={() => readFile(item)}>
									{prettyPrint(item)}
								</button>
							</li>
						{/each}
					</ul>
				</details>
			{:else if label === 'host_ipc' || label === 'host_network' || label === 'host_pid'}
				{#if data === 'Yes' || data === true}
					<div class="mb-1 flex items-center gap-1" class:field-changed={highlightedFields[label]}>
						<span class="badge bg-error-100 text-error-800 dark:bg-error-900 dark:text-error-200 text-xs font-bold">{label}</span>
						<Icon icon="mdi:alert" width="14" class="text-error-500" />
					</div>
				{:else}
					<div class="mb-1 text-surface-400 dark:text-surface-600" class:field-changed={highlightedFields[label]}>
						<span class="font-semibold mr-1">{label}:</span>{data}
					</div>
				{/if}
			{:else if label === 'owner_references' && Array.isArray(data) && data.length > 0}
				<div class="mb-1" class:field-changed={highlightedFields[label]}>
					<span class="font-bold mr-1">Owner:</span>
					{#each data as oref}
						<span class="inline-flex items-center gap-1">
							<span class="badge bg-indigo-100 text-indigo-800 text-xs">{oref.kind}</span>
							<span class="font-mono text-xs">{oref.name}</span>
						</span>
					{/each}
				</div>
			{:else if Array.isArray(data) && data.length > 0}
				{#if data.length === 1}
					<div class="mb-1 flex items-center gap-1" class:field-changed={highlightedFields[label]}><span class="font-bold mr-1">{label}:</span>{prettyPrint(data[0])}{@render runBtn(label)}</div>
				{:else}
				<details class="mb-1" class:field-changed={highlightedFields[label]}>
					<summary class="flex items-center gap-1">
						<span class="font-bold">{label}</span>
						<span class="text-xs text-surface-500">({data.length})</span>
						{@render runBtn(label)}
					</summary>
					<ul class="list-inside list-none pl-5">
						{#each data as item}
							<li>{prettyPrint(item)}</li>
						{/each}
					</ul>
				</details>
				{/if }
			{:else if (label === 'binaries' || label === 'envVars') && typeof data === 'object' && data !== null}
				{@const dictEmpty = Object.keys(data).length === 0}
				<!-- Special formatting for binaries and envVars dictionary -->
				<details class="mb-1" class:field-changed={highlightedFields[label]}>
					<summary class="flex items-center gap-1">
						<span class:font-bold={!dictEmpty} class:text-surface-400={dictEmpty} class:opacity-40={dictEmpty}>{label}</span>
						<span class="text-xs text-surface-500" class:opacity-40={dictEmpty}>({Object.keys(data).length})</span>
						{@render runBtn(label)}
					</summary>
					<ul class="list-inside list-none pl-5">
						{#each Object.entries(data).sort(([a], [b]) => a.localeCompare(b)) as [key, value]}
							<li class="font-mono text-sm">
								<span class="font-semibold">{key}:</span>
								{#if label === 'binaries' && value === ''}
									<span class="text-error-500 font-semibold">absent</span>
								{:else if value === ''}
									<span class="text-surface-400 italic">empty</span>
								{:else}
									{value}
								{/if}
							</li>
						{/each}
					</ul>
				</details>
			{:else if label === 'meta' && typeof data === 'object' && data !== null}
				{@const uid = data.uid}
				{@const createdAt = data.created_at}
				{@const labels = data.labels && Object.keys(data.labels).length > 0 ? data.labels : null}
				{@const annotations = data.annotations && Object.keys(data.annotations).length > 0 ? data.annotations : null}
				{@const owner = data.owner ?? null}
				{#if uid || createdAt || labels || annotations || owner}
					<div class="mb-1 space-y-0.5" class:field-changed={highlightedFields[label]}>
						{#if createdAt}
							<div><span class="font-semibold mr-1">Created:</span>{createdAt}</div>
						{/if}
						{#if uid}
							<div><span class="font-semibold mr-1">UID:</span><span class="font-mono text-xs">{uid}</span></div>
						{/if}
						{#if owner}
							<div>
								<span class="font-semibold mr-1">Owner:</span>
								<span class="badge bg-indigo-100 text-indigo-800 text-xs">{owner.kind}</span>
								<span class="font-mono text-xs ml-1">{owner.name}</span>
							</div>
						{/if}
						{#if labels}
							<details>
								<summary class="cursor-pointer">
									<span class="font-semibold">Labels</span>
									<span class="text-xs text-surface-500">({Object.keys(labels).length})</span>
								</summary>
								<ul class="pl-4 list-none font-mono text-xs space-y-0.5 mt-1">
									{#each Object.entries(labels).sort(([a], [b]) => a.localeCompare(b)) as [k, v]}
										<li><span class="text-surface-500">{k}=</span>{v}</li>
									{/each}
								</ul>
							</details>
						{/if}
						{#if annotations}
							<details>
								<summary class="cursor-pointer">
									<span class="font-semibold">Annotations</span>
									<span class="text-xs text-surface-500">({Object.keys(annotations).length})</span>
								</summary>
								<ul class="pl-4 list-none font-mono text-xs space-y-0.5 mt-1">
									{#each Object.entries(annotations).sort(([a], [b]) => a.localeCompare(b)) as [k, v]}
										<li><span class="text-surface-500">{k}=</span>{v}</li>
									{/each}
								</ul>
							</details>
						{/if}
					</div>
				{/if}
			{:else if label === 'token' && obj.kind === 'ServiceAccount' && typeof data === 'object' && data !== null && data.Raw}
				<!-- Special handling for ServiceAccount token with copy button -->
				<div class="mb-1 flex items-center gap-2" class:field-changed={highlightedFields[label]}>
					<details class="mb-1" class:field-changed={highlightedFields[label]}>
						<summary>
							<span class="font-bold">{label}</span>
							<span class="text-xs text-surface-500">({Array.isArray(data) ? data.length : Object.keys(data).length})</span>
						</summary>
						<pre class="max-h-80 overflow-scroll">{JSON.stringify(data, null, 2)}</pre>
					</details>
						<button
							class="shrink-0 cursor-pointer rounded p-0.5 hover:bg-surface-300 dark:hover:bg-surface-700 transition-colors"
							title="Copy token"
							onclick={copyToken}
						>
							{#if tokenCopied}
								<Icon icon="mdi:check" width="16" class="text-success-500" />
							{:else}
								<Icon icon="mdi:content-copy" width="16" class="text-surface-500" />
							{/if}
						</button>
				</div>
			{:else if typeof data === 'object' && data !== null}
				{@const isEmpty = Array.isArray(data) ? data.length === 0 : Object.keys(data).length === 0}
				<details class="mb-1" class:field-changed={highlightedFields[label]}>
					<summary class="flex items-center gap-1">
						<span class:font-bold={!isEmpty} class:text-surface-400={isEmpty} class:opacity-40={isEmpty}>{label}</span>
						<span class="text-xs text-surface-500 " class:opacity-40={isEmpty}>({Array.isArray(data) ? data.length : Object.keys(data).length})</span>
						{@render runBtn(label)}
					</summary>
					<pre class="max-h-80 overflow-scroll" class:opacity-40={isEmpty}>{JSON.stringify(data, null, 2)}</pre>
				</details>
			{:else if data !== undefined}
				<div class="mb-1 flex items-center gap-1" class:field-changed={highlightedFields[label]}>
					<span class="font-bold mr-1">{label}:</span>{prettyPrint(data)}
					{@render runBtn(label)}
				</div>
			{/if}
		{/each}
		<!-- Placeholder rows for discoverable fields not yet present on the entity -->
		{#each [...fieldTtpIndex.entries()].filter(([field]) => !(field in (obj ?? {}))) as [field, ttp]}
			{#if sendAction}
				<div class="mb-1 flex items-center gap-1">
					<span class="opacity-40 text-surface-400 mr-1">{field}:</span>
					<span class="opacity-40 italic text-surface-400">—</span>
					<button
						class="shrink-0 cursor-pointer rounded p-0.5 hover:bg-surface-300 dark:hover:bg-surface-700 transition-colors"
						title="Run: {ttp.name}"
						onclick={() => sendAction!(ttp, {})}
					>
						<Icon icon="mdi:play-circle-outline" width="14" class="text-primary-500" />
					</button>
				</div>
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
