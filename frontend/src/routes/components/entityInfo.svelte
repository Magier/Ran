<script lang="ts">
	import EntitlementInfo from './entitlement_info.svelte';
	import Icon from '@iconify/svelte';
	import type { RBACPermission, TTP } from '$lib/api/index';
	import { showToast } from '$lib/components/toaster';
	import { getCampaignState } from '$lib/components/CampaignState.svelte';
	import { knowledgeProvenanceBadges } from '$lib/knowledgeProvenance';
	import { WORKLOAD_KINDS } from './workload_compounds';

	type ObjectInfoProps = {
		objectId: string;
		sendAction?: (ttp: TTP, args: any) => void;
		class: string | undefined;
	};

	let { objectId, sendAction, class: className }: ObjectInfoProps = $props();

	const campaignState = getCampaignState();
	const obj = $derived(campaignState.getObjectById(objectId));

	// Fields where the button should be suppressed for specific entity kinds.
	const FIELD_KIND_EXCLUDE: Record<string, string[]> = {
		service_account_name: ['ServiceAccount']
	};

	const EFFECT_FIELD_MAP: Record<string, string[]> = {
		'linux.mounts': ['mounts'],
		'sys.envVar': ['envVars'],
		'sys.ip': ['ips'],
		'sys.files': ['files', 'binaries'],
		'sys.userID': ['user_id'],
		rawServiceaccountToken: ['service_account_name'],
		'k8s.SelfSubjectRulesReview': ['can']
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
		// Track accessLevel and compromised so the TTP list refreshes when
		// exec access is gained (e.g. after an exec relation is created).
		const _track = obj?.accessLevel;
		const _track2 = obj?.compromised;
		if (!id) {
			applicableTtps = [];
			return;
		}
		const kind = campaignState.graph.nodes.find((node) => node.id === id)?.kind;
		const podIds =
			kind && WORKLOAD_KINDS.has(kind)
				? campaignState.graph.nodes
						.filter((node) => node.kind === 'Pod' && node.parent === id)
						.map((node) => node.id)
				: [];
		const request =
			podIds.length > 0
				? Promise.all(podIds.map((podId) => campaignState.api.GetApplicableTTPs(podId))).then(
						(results) => {
							const byId = new Map<string, TTP>();
							results.flat().forEach((ttp) => byId.set(ttp.id, ttp));
							return [...byId.values()];
						}
					)
				: campaignState.api.GetApplicableTTPs(id);
		request
			.then((ttps) => {
				applicableTtps = ttps;
			})
			.catch(() => {
				applicableTtps = [];
			});
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
	let canExpanded = $state(false);
	let timeouts: Map<string, number> = new Map();

	// Track changes in object fields
	$effect(() => {
		if (!obj) return;

		// Check if the objectId changed (user selected a different entity)
		if (previousObjectId !== objectId) {
			// Different entity selected - reset without highlighting
			previousObjectId = objectId;
			previousValues = {};
			canExpanded = false;
			for (const [key, value] of Object.entries(obj)) {
				previousValues[key] = value;
			}
			// Clear all highlights
			highlightedFields = {};
			timeouts.forEach((timeout) => clearTimeout(timeout));
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
			timeouts.forEach((timeout) => clearTimeout(timeout));
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
		} else if (obj != null && Object.hasOwn(obj, 'IP')) {
			// Handle special case for objects with 'IP' property
			return obj.IP;
		} else {
			return JSON.stringify(obj, null, 2);
		}
	}

	// Fields handled explicitly in the header - skip from the generic loop
	const HEADER_FIELDS = new Set([
		'id',
		'name',
		'namespace',
		'kind',
		'entityId',
		'parent',
		'entity',
		'compromised',
		'provenance',
		'appServiceCount'
	]);

	function shouldShowField(label: string, data: any): boolean {
		if (HEADER_FIELDS.has(label)) return false;
		if (data === undefined) return false;
		// Hide running state when positive - it's the default and duplicates phase
		if ((label === 'isRunning' || label === 'is_running') && data !== false) return false;
		// Hide phase: Running - same info as is_running: true
		if (label === 'phase' && data === 'Running') return false;
		// Hide empty owner_references
		if (label === 'owner_references' && Array.isArray(data) && data.length === 0) return false;
		// An explicit empty `can` means the permission review completed with no rules.
		if (label === 'can' && Array.isArray(data)) return true;
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
			setTimeout(() => {
				idCopied = false;
			}, 1500);
		});
	}

	function copyToken() {
		if (!obj || !obj.token || !obj.token.Raw) return;
		navigator.clipboard.writeText(obj.token.Raw).then(() => {
			tokenCopied = true;
			setTimeout(() => {
				tokenCopied = false;
			}, 1500);
		});
	}

	function ttpForField(label: string): TTP | undefined {
		const excludedKinds = FIELD_KIND_EXCLUDE[label] ?? [];
		if (obj?.kind && excludedKinds.includes(obj.kind)) return undefined;
		return fieldTtpIndex.get(label);
	}

	function readFile(path: string) {
		const ttp = campaignState.getTtpById('read-file');
		if (ttp) {
			if (sendAction) {
				sendAction(ttp, { PATH: path });
			} else {
				showToast('No sendAction function provided', '', 'error');
			}
		} else {
			showToast("TTP 'read-file' not found", '', 'error');
		}
	}
</script>

<!-- specify data-popup attr. for consistent styling via skeleton-ui -->
<!-- class="card variant-filled-secondary details-popup bg-surface-50-950 z-100 flex w-96 flex-col overflow-auto p-4 {selectedNode  -->
<div
	class="{className} border-surface-600 bg-surface-100-900 pointer-events-auto w-full overflow-auto rounded-lg border p-4 text-xs shadow-xl md:text-sm"
>
	{#if obj}
		{#snippet runBtn(label: string)}
			{@const ttp = ttpForField(label)}
			{#if ttp && sendAction}
				<button
					class="hover:bg-surface-300 dark:hover:bg-surface-700 shrink-0 cursor-pointer rounded p-0.5 transition-colors"
					title="Run: {ttp.name}"
					onclick={() => sendAction!(ttp!, {})}
				>
					<Icon icon="mdi:play-circle-outline" width="14" class="text-primary-500" />
				</button>
			{/if}
		{/snippet}
		<!-- Header: name + kind badge + copy-ID button -->
		<div class="mb-1 flex items-center gap-2">
			<span
				class="truncate text-sm font-bold md:text-base"
				class:field-changed={highlightedFields['name']}
				>{obj.name}{#if obj.meta?.name_confidence === 'derived'}<sup
						class="text-surface-400 dark:text-surface-600 cursor-help"
						title="Name is derived - inferred from heuristics or indirect sources, not confirmed by the Kubernetes API"
						>*</sup
					>{/if}</span
			>
			{#if obj.kind}
				<span class="badge shrink-0 bg-indigo-200 text-xs text-indigo-800">{obj.kind}</span>
			{/if}
			{#each knowledgeProvenanceBadges(obj.provenance) as badge}
				<span
					class="badge shrink-0 text-xs"
					class:bg-amber-200={badge.origin === 'scenario'}
					class:text-amber-900={badge.origin === 'scenario'}
				>
					{badge.label}
				</span>
			{/each}
			<button
				class="hover:bg-surface-300 dark:hover:bg-surface-700 shrink-0 cursor-pointer rounded p-0.5 transition-colors"
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
				<span class="mr-1 font-semibold">Namespace:</span>{obj.namespace}
			</div>
		{/if}

		{#each Object.entries(obj || {})
			.filter(([label, data]) => shouldShowField(label, data))
			.sort(([a], [b]) => a.localeCompare(b)) as [label, data]}
			{#if label === 'containers' && Array.isArray(data) && data.length > 0}
				<!-- Special drill-down view for containers -->
				<details class="mb-1" class:field-changed={highlightedFields[label]}>
					<summary>
						<span class="font-bold">{label}</span>
						<span class="text-surface-500 text-xs">({data.length})</span>
					</summary>
					<div class="space-y-4 pl-4">
						{#each data as container}
							<div class="border-surface-500 border-l-2 py-2 pl-3">
								<!-- Top: name and command -->
								<span>Name: </span>
								<div class="font-bold">{container.name}</div>
								{#if container.command && container.command.length > 0}
									<div class="mt-1">
										<span class="text-surface-600 dark:text-surface-400 text-sm">Command:</span>
										<code class="ml-1 text-xs">{container.command.join(' ')}</code>
									</div>
								{/if}
								{#if container.args && container.args.length > 0}
									<div class="mt-1">
										<span class="text-surface-600 dark:text-surface-400 text-sm">Args:</span>
										<code class="ml-1 text-xs">{container.args.join(' ')}</code>
									</div>
								{/if}

								<!-- Second level: volumeMounts and ports -->
								{#if container.volume_mounts && container.volume_mounts.length > 0}
									<details class="mt-2">
										<summary class="text-surface-600 dark:text-surface-400 cursor-pointer text-sm">
											Volume Mounts ({container.volume_mounts.length})
										</summary>
										<ul class="mt-1 list-inside list-none space-y-0.5 pl-4">
											{#each container.volume_mounts as vm}
												<li class="flex flex-wrap items-center gap-1 text-xs">
													<span class="font-mono">{vm.mount_point}</span>
													<span class="text-surface-400">({vm.name})</span>
													{#if vm.read_only}<span
															class="badge bg-warning-100 text-warning-800 text-xs">ro</span
														>{/if}
													{#if vm.is_host_path}<span
															class="badge bg-error-100 text-error-800 text-xs"
															>hostPath: {vm.mount_root}</span
														>{/if}
												</li>
											{/each}
										</ul>
									</details>
								{/if}

								{#if container.ports && container.ports.length > 0}
									<details class="mt-2">
										<summary class="text-surface-600 dark:text-surface-400 cursor-pointer text-sm">
											Ports ({container.ports.length})
										</summary>
										<ul class="mt-1 list-inside list-disc pl-4 text-sm">
											{#each container.ports as port}
												<li>
													{#if port.name}<span class="font-mono">{port.name}:</span>
													{/if}
													<span class="font-mono"
														>{port.containerPort}/{port.protocol || 'TCP'}</span
													>
													{#if port.hostPort}
														→ <span class="font-mono">{port.hostPort}</span>{/if}
												</li>
											{/each}
										</ul>
									</details>
								{/if}

								<!-- Rest of properties -->
								{#if container.image}
									<div class="mt-2 text-sm">
										<span class="text-surface-600 dark:text-surface-400">Image:</span>
										<span class="ml-1 font-mono text-xs">{container.image}</span>
									</div>
								{/if}

								{#if container.env && container.env.length > 0}
									<details class="mt-2">
										<summary class="text-surface-600 dark:text-surface-400 cursor-pointer text-sm">
											Environment ({container.env.length})
										</summary>
										<ul class="mt-1 list-inside list-none pl-4 font-mono text-xs">
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
										<summary class="text-surface-600 dark:text-surface-400 cursor-pointer text-sm">
											Security Context
										</summary>
										<pre class="mt-1 overflow-auto pl-4 text-xs">{JSON.stringify(
												container.securityContext,
												null,
												2
											)}</pre>
									</details>
								{/if}

								{#if container.resources}
									<details class="mt-2">
										<summary class="text-surface-600 dark:text-surface-400 cursor-pointer text-sm">
											Resources
										</summary>
										<pre class="mt-1 overflow-auto pl-4 text-xs">{JSON.stringify(
												container.resources,
												null,
												2
											)}</pre>
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
						<span class="text-surface-500 text-xs">({data.length})</span>
					</summary>
					<ul class="mt-1 list-inside list-none space-y-1 pl-4">
						{#each data as m}
							<li class="flex flex-wrap items-center gap-1">
								<span class="font-mono text-xs">{m.mount_point ?? m.mountPath}</span>
								{#if m.name}
									<span class="text-surface-400 text-xs">({m.name})</span>
								{/if}
								{#if m.read_only || m.readOnly}
									<span class="badge bg-warning-100 text-warning-800 text-xs">ro</span>
								{/if}
								{#if m.is_host_path}
									<span class="badge bg-error-100 text-error-800 text-xs"
										>hostPath: {m.mount_root}</span
									>
								{/if}
							</li>
						{/each}
					</ul>
				</details>
			{:else if label === 'can'}
				{#if typeof data === 'object' && data !== null && Object.keys(data).length > 0}
					<details
						class="mb-1"
						class:field-changed={highlightedFields[label]}
						bind:open={canExpanded}
					>
						<summary class="flex items-center gap-1">
							<span class="font-bold">{label}</span>
							<span class="text-surface-500 text-xs"
								>({Array.isArray(data) ? data.length : Object.keys(data).length})</span
							>
							{#if campaignState.kubetier && canExpanded}
								<span class="group relative inline-flex items-center">
									<button
										type="button"
										class="text-surface-500 hover:text-surface-700 focus:ring-primary-500 dark:hover:text-surface-200 inline-flex cursor-help items-center rounded-sm focus:ring-1 focus:outline-none"
										aria-label="About KubeTier criticality assessment"
										onclick={(event) => event.stopPropagation()}
									>
										<Icon icon="mdi:information-outline" width="15" />
									</button>
									<span
										class="invisible absolute top-full left-0 z-30 w-72 pt-1 opacity-0 transition-opacity group-focus-within:visible group-focus-within:opacity-100 group-hover:visible group-hover:opacity-100"
									>
										<span
											role="tooltip"
											class="border-surface-300 bg-surface-50 text-surface-700 dark:border-surface-600 dark:bg-surface-900 dark:text-surface-200 block rounded-md border p-3 text-left text-xs font-normal shadow-lg"
										>
											<span class="mb-1 block font-semibold">Permission criticality</span>
											<span class="block">
												<span class="font-semibold text-red-700 dark:text-red-300">Red T0</span>
												(highest) ·
												<span class="font-semibold text-orange-700 dark:text-orange-300"
													>orange T1</span
												>
												·
												<span class="text-green-700 dark:text-green-300">green T2</span> ·
												<span class="text-surface-500 dark:text-surface-400">gray T3</span>
												(lowest).
												<a
													class="text-primary-700 dark:text-primary-300 ml-1 font-semibold underline"
													href="https://kubetier.com/"
													target="_blank"
													rel="noreferrer"
													onclick={(event) => event.stopPropagation()}>Informed by KubeTier ↗</a
												>
											</span>
										</span>
									</span>
								</span>
							{/if}
						</summary>
						<EntitlementInfo
							entitlements={data as RBACPermission[]}
							catalog={campaignState.kubetier}
						/>
					</details>
				{:else}
					<div class="mb-1" class:field-changed={highlightedFields[label]}>
						<span class="mr-1 font-bold">{label}</span>none
					</div>
					<!-- <button class="btn btn-sm preset-filled-primary-500" disabled>🔍</button> -->
				{/if}
			{:else if label === 'permissions' && Array.isArray(data) && data.length > 0 && (obj.kind === 'Role' || obj.kind === 'ClusterRole')}
				<details class="mb-1" class:field-changed={highlightedFields[label]} open>
					<summary>
						<span class="font-bold">permissions</span>
						<span class="text-surface-500 text-xs">({data.length})</span>
					</summary>
					<EntitlementInfo
						entitlements={data as RBACPermission[]}
						catalog={campaignState.kubetier}
						roleName={obj.name}
						roleKind={obj.kind}
					/>
				</details>
			{:else if label === 'files' && Array.isArray(data) && data.length > 0}
				<details class="mb-1" class:field-changed={highlightedFields[label]}>
					<summary>
						<span class="font-bold">{label}</span>
						<span class="text-surface-500 text-xs">({data.length})</span>
					</summary>
					<ul class="list-inside list-none pl-5">
						{#each data as item}
							<li>
								<button
									class="cursor-pointer text-left hover:underline"
									onclick={() => readFile(item)}
								>
									{prettyPrint(item)}
								</button>
							</li>
						{/each}
					</ul>
				</details>
			{:else if label === 'host_ipc' || label === 'host_network' || label === 'host_pid'}
				{#if data === 'Yes' || data === true}
					<div class="mb-1 flex items-center gap-1" class:field-changed={highlightedFields[label]}>
						<span
							class="badge bg-error-100 text-error-800 dark:bg-error-900 dark:text-error-200 text-xs font-bold"
							>{label}</span
						>
						<Icon icon="mdi:alert" width="14" class="text-error-500" />
					</div>
				{:else}
					<div
						class="text-surface-400 dark:text-surface-600 mb-1"
						class:field-changed={highlightedFields[label]}
					>
						<span class="mr-1 font-semibold">{label}:</span>{data}
					</div>
				{/if}
			{:else if label === 'owner_references' && Array.isArray(data) && data.length > 0}
				<div class="mb-1" class:field-changed={highlightedFields[label]}>
					<span class="mr-1 font-bold">Owner:</span>
					{#each data as oref}
						<span class="inline-flex items-center gap-1">
							<span class="badge bg-indigo-100 text-xs text-indigo-800">{oref.kind}</span>
							<span class="font-mono text-xs">{oref.name}</span>
						</span>
					{/each}
				</div>
			{:else if label === 'sessions' && Array.isArray(data) && data.length > 0}
				<details class="mb-1" class:field-changed={highlightedFields[label]}>
					<summary>
						<span class="font-bold">{label}</span>
						<span class="text-surface-500 text-xs">({data.length})</span>
					</summary>
					<ul class="mt-1 list-inside list-none space-y-1 pl-4">
						{#each data as session}
							<li class="flex flex-wrap items-center gap-1 text-xs">
								<span
									class="badge text-xs {session.status === 'Active'
										? 'bg-success-100 text-success-800'
										: session.status === 'Lost'
											? 'bg-error-100 text-error-800'
											: 'bg-warning-100 text-warning-800'}">{session.status}</span
								>
								<span class="font-mono">{session.kind}</span>
								{#if session.port}
									<span class="text-surface-400">:{session.port}</span>
								{/if}
								<span class="text-surface-400 truncate font-mono">{session.id}</span>
							</li>
						{/each}
					</ul>
				</details>
			{:else if label === 'appServices' && Array.isArray(data)}
				<details class="mb-1" class:field-changed={highlightedFields[label]}>
					<summary class="cursor-pointer">
						<span class="font-bold">Services</span>
						<span class="text-surface-500 text-xs">({data.length})</span>
					</summary>
					<div class="mt-1 space-y-1 pl-4">
						{#each data as service}
							<details>
								<summary class="cursor-pointer">
									<span class="font-mono font-semibold">{service.port}/{service.transport}</span>
									{#if service.port_name}
										<span class="text-surface-500 ml-1">· {service.port_name}</span>
									{/if}
									{#if service.product && service.product !== service.port_name}
										<span class="text-surface-500 ml-1">· {service.product}?</span>
									{/if}
								</summary>
								<dl class="grid grid-cols-[auto_1fr] gap-x-2 pl-4 text-xs">
									<dt class="font-semibold">Address</dt>
									<dd class="font-mono">{service.address}</dd>
									<dt class="font-semibold">Endpoint</dt>
									<dd class="font-mono">{service.port}/{service.transport}</dd>
									<dt class="font-semibold">State</dt>
									<dd class="capitalize">{service.state}</dd>
									{#if service.port_name}<dt class="font-semibold">Name</dt>
										<dd>{service.port_name}</dd>{/if}
									{#if service.product}<dt class="font-semibold">Assume</dt>
										<dd>
											{service.product}{#if service.version}
												{service.version}{/if}
										</dd>{/if}
									{#if service.banner}<dt class="font-semibold">Banner</dt>
										<dd>{service.banner}</dd>{/if}
									{#if service.cpes?.length}<dt class="font-semibold">CPE</dt>
										<dd>{service.cpes.join(', ')}</dd>{/if}
								</dl>
							</details>
						{/each}
					</div>
				</details>
			{:else if Array.isArray(data) && data.length > 0}
				{#if data.length === 1}
					<div class="mb-1 flex items-center gap-1" class:field-changed={highlightedFields[label]}>
						<span class="mr-1 font-bold">{label}:</span>{prettyPrint(data[0])}{@render runBtn(
							label
						)}
					</div>
				{:else}
					<details class="mb-1" class:field-changed={highlightedFields[label]}>
						<summary class="flex items-center gap-1">
							<span class="font-bold">{label}</span>
							<span class="text-surface-500 text-xs">({data.length})</span>
							{@render runBtn(label)}
						</summary>
						<ul class="list-inside list-none pl-5">
							{#each data as item}
								<li>{prettyPrint(item)}</li>
							{/each}
						</ul>
					</details>
				{/if}
			{:else if (label === 'binaries' || label === 'envVars') && typeof data === 'object' && data !== null}
				{@const dictEmpty = Object.keys(data).length === 0}
				<!-- Special formatting for binaries and envVars dictionary -->
				<details class="mb-1" class:field-changed={highlightedFields[label]}>
					<summary>
						<span class="inline-flex items-center gap-1">
							<span
								class:font-bold={!dictEmpty}
								class:text-surface-400={dictEmpty}
								class:opacity-40={dictEmpty}>{label}</span
							>
							<span class="text-surface-500 text-xs" class:opacity-40={dictEmpty}
								>({Object.keys(data).length})</span
							>
							{@render runBtn(label)}
						</span>
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
				{@const annotations =
					data.annotations && Object.keys(data.annotations).length > 0 ? data.annotations : null}
				{@const owner = data.owner ?? null}
				{#if uid || createdAt || labels || annotations || owner}
					<div class="mb-1 space-y-0.5" class:field-changed={highlightedFields[label]}>
						{#if createdAt}
							<div><span class="mr-1 font-semibold">Created:</span>{createdAt}</div>
						{/if}
						{#if uid}
							<div>
								<span class="mr-1 font-semibold">UID:</span><span class="font-mono text-xs"
									>{uid}</span
								>
							</div>
						{/if}
						{#if owner}
							<div>
								<span class="mr-1 font-semibold">Owner:</span>
								<span class="badge bg-indigo-100 text-xs text-indigo-800">{owner.kind}</span>
								<span class="ml-1 font-mono text-xs">{owner.name}</span>
							</div>
						{/if}
						{#if labels}
							<details>
								<summary class="cursor-pointer">
									<span class="font-semibold">Labels</span>
									<span class="text-surface-500 text-xs">({Object.keys(labels).length})</span>
								</summary>
								<ul class="mt-1 list-none space-y-0.5 pl-4 font-mono text-xs">
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
									<span class="text-surface-500 text-xs">({Object.keys(annotations).length})</span>
								</summary>
								<ul class="mt-1 list-none space-y-0.5 pl-4 font-mono text-xs">
									{#each Object.entries(annotations).sort( ([a], [b]) => a.localeCompare(b) ) as [k, v]}
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
							<span class="text-surface-500 text-xs"
								>({Array.isArray(data) ? data.length : Object.keys(data).length})</span
							>
						</summary>
						<pre class="max-h-80 overflow-scroll">{JSON.stringify(data, null, 2)}</pre>
					</details>
					<button
						class="hover:bg-surface-300 dark:hover:bg-surface-700 shrink-0 cursor-pointer rounded p-0.5 transition-colors"
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
						<span
							class:font-bold={!isEmpty}
							class:text-surface-400={isEmpty}
							class:opacity-40={isEmpty}>{label}</span
						>
						<span class="text-surface-500 text-xs" class:opacity-40={isEmpty}
							>({Array.isArray(data) ? data.length : Object.keys(data).length})</span
						>
						{@render runBtn(label)}
					</summary>
					<pre class="max-h-80 overflow-scroll" class:opacity-40={isEmpty}>{JSON.stringify(
							data,
							null,
							2
						)}</pre>
				</details>
			{:else if data !== undefined}
				<div class="mb-1 flex items-center gap-1" class:field-changed={highlightedFields[label]}>
					<span class="mr-1 font-bold">{label}:</span>{prettyPrint(data)}
					{@render runBtn(label)}
				</div>
			{/if}
		{/each}
		<!-- Placeholder rows for discoverable fields not yet present on the entity -->
		{#each [...fieldTtpIndex.entries()].filter(([field]) => !(field in (obj ?? {}))) as [field]}
			{@const ttp = ttpForField(field)}
			{#if ttp && sendAction}
				<div class="mb-1 flex items-center gap-1">
					<span class="text-surface-400 mr-1 opacity-40">{field}:</span>
					<span class="text-surface-400 italic opacity-40">-</span>
					<button
						class="hover:bg-surface-300 dark:hover:bg-surface-700 shrink-0 cursor-pointer rounded p-0.5 transition-colors"
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
			<span class="mr-1 font-bold">ID</span>
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
