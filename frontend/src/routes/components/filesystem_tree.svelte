<script lang="ts">
	import { onMount } from 'svelte';
	import Icon from '@iconify/svelte';
	import { showToast } from '$lib/components/toaster';
	import { getCampaignState } from '$lib/components/CampaignState.svelte';
	import {
		filesystemChildren,
		filesystemVolumeMountsAt,
		type FilesystemVolumeMount
	} from './filesystem_tree';

	type Props = {
		objectId: string;
		files?: string[];
		directories?: string[];
		listedDirectories?: string[];
		volumeMounts?: FilesystemVolumeMount[];
		canList: boolean;
	};

	let {
		objectId,
		files = [],
		directories = [],
		listedDirectories = [],
		volumeMounts = [],
		canList
	}: Props = $props();
	const campaignState = getCampaignState();
	let expandedDirectories = $state(new Set<string>());
	let loadingDirectories = $state(new Set<string>());
	let requestedDirectories = $state(new Set<string>());
	let previousObjectId = '';

	$effect(() => {
		if (objectId === previousObjectId) return;
		previousObjectId = objectId;
		expandedDirectories = new Set();
		loadingDirectories = new Set();
		requestedDirectories = new Set();
	});

	function children(path: string) {
		return filesystemChildren(
			path,
			files,
			directories,
			volumeMounts.map((mount) => mount.mountPoint)
		);
	}

	function isListed(path: string): boolean {
		return listedDirectories.includes(path);
	}

	function mountsAt(path: string): FilesystemVolumeMount[] {
		return filesystemVolumeMountsAt(path, volumeMounts);
	}

	function mountTitle(path: string, mounts: FilesystemVolumeMount[]): string {
		const details = mounts.map((mount) => {
			const source = mount.isHostPath && mount.hostPath ? ` from host ${mount.hostPath}` : '';
			const context = mount.container ? ` on ${mount.container}` : '';
			const type = mount.mountType ? ` (${mount.mountType})` : '';
			const name = mount.name || (mount.origin === 'runtime' ? 'runtime mount' : 'volume');
			return `${name}${context}${source}${type}${mount.readOnly ? ' (read-only)' : ''}`;
		});
		return [path, ...details].join('\n');
	}

	async function toggleDirectory(path: string) {
		if (expandedDirectories.has(path)) {
			const next = new Set(expandedDirectories);
			next.delete(path);
			expandedDirectories = next;
			return;
		}

		expandedDirectories = new Set(expandedDirectories).add(path);
		if (requestedDirectories.has(path) || loadingDirectories.has(path) || !canList) return;

		requestedDirectories = new Set(requestedDirectories).add(path);
		loadingDirectories = new Set(loadingDirectories).add(path);
		try {
			await campaignState.api.ExecuteAction({
				actionId: 'list-files',
				targetId: objectId,
				args: { DIR: path },
				executionTimeoutSeconds: 60
			});
		} catch (error) {
			const requested = new Set(requestedDirectories);
			requested.delete(path);
			requestedDirectories = requested;
			const next = new Set(loadingDirectories);
			next.delete(path);
			loadingDirectories = next;
			showToast(
				'Unable to list directory',
				error instanceof Error ? error.message : String(error),
				'error'
			);
		}
	}

	async function readFile(path: string) {
		try {
			await campaignState.api.ExecuteAction({
				actionId: 'read-file',
				targetId: objectId,
				args: { PATH: path },
				executionTimeoutSeconds: 60
			});
		} catch (error) {
			showToast(
				'Unable to read file',
				error instanceof Error ? error.message : String(error),
				'error'
			);
		}
	}

	function actionFinished(data: any) {
		if (
			data?.TargetID !== objectId ||
			data?.TTP?.id !== 'list-files' ||
			typeof data?.Args?.DIR !== 'string'
		) {
			return;
		}
		const next = new Set(loadingDirectories);
		next.delete(data.Args.DIR);
		loadingDirectories = next;
	}

	onMount(() => {
		campaignState.api.on('ttp-executed', actionFinished);
		return () => campaignState.api.off('ttp-executed', actionFinished);
	});
</script>

{#snippet directory(path: string, name: string, depth: number)}
	{@const mountedVolumes = mountsAt(path)}
	{@const showMountMarker = path !== '/' && mountedVolumes.length > 0}
	{@const isHostPathMount = mountedVolumes.some((mount) => mount.isHostPath)}
	{@const hasConfiguredMount = mountedVolumes.some((mount) => mount.origin === 'configured')}
	<div>
		<button
			type="button"
			class="hover:bg-surface-300 dark:hover:bg-surface-700 flex w-full cursor-pointer items-center rounded py-0.5 pr-1 text-left"
			style:padding-left={`${depth * 0.75}rem`}
			onclick={() => toggleDirectory(path)}
			title={mountedVolumes.length > 0
				? mountTitle(path, mountedVolumes)
				: canList || isListed(path)
					? path
					: 'Directory listing is not currently applicable'}
		>
			{#if loadingDirectories.has(path)}
				<Icon icon="mdi:loading" width="14" class="mr-1 shrink-0 animate-spin" />
			{:else}
				<Icon
					icon={expandedDirectories.has(path) ? 'mdi:chevron-down' : 'mdi:chevron-right'}
					width="14"
					class="mr-1 shrink-0"
				/>
			{/if}
			{#if showMountMarker && (isHostPathMount || hasConfiguredMount)}
				<Icon
					icon="mdi:folder-swap-outline"
					width="15"
					class={`mr-1 shrink-0 ${isHostPathMount ? 'text-red-500' : 'text-indigo-500'}`}
				/>
			{:else if showMountMarker}
				<Icon icon="mdi:harddisk" width="15" class="mr-1 shrink-0 text-cyan-600" />
			{:else}
				<Icon
					icon={expandedDirectories.has(path) ? 'mdi:folder-open' : 'mdi:folder'}
					width="15"
					class="mr-1 shrink-0 text-amber-500"
				/>
			{/if}
			<span class="truncate font-mono text-xs">{name}</span>
			{#if showMountMarker && (isHostPathMount || hasConfiguredMount)}
				<span
					class="badge ml-1 text-[10px]"
					class:bg-red-100={isHostPathMount}
					class:text-red-800={isHostPathMount}
					class:bg-indigo-100={!isHostPathMount}
					class:text-indigo-800={!isHostPathMount}
				>
					{isHostPathMount ? 'hostPath' : 'volume'}
				</span>
				{#if mountedVolumes.every((mount) => mount.readOnly)}
					<span class="badge bg-warning-100 text-warning-800 ml-1 text-[10px]">ro</span>
				{/if}
			{/if}
		</button>
		{#if expandedDirectories.has(path)}
			{@const entries = children(path)}
			{#if isListed(path) && entries.length === 0}
				<div
					class="text-surface-400 py-0.5 text-xs italic"
					style:padding-left={`${(depth + 2) * 0.75}rem`}
				>
					empty
				</div>
			{/if}
			{#each entries as entry (entry.path)}
				{#if entry.kind === 'directory'}
					{@render directory(entry.path, entry.name, depth + 1)}
				{:else}
					<button
						type="button"
						class="hover:bg-surface-300 dark:hover:bg-surface-700 flex w-full cursor-pointer items-center rounded py-0.5 pr-1 text-left"
						style:padding-left={`${(depth + 1) * 0.75 + 1.25}rem`}
						onclick={() => readFile(entry.path)}
						title={`Read ${entry.path}`}
					>
						<Icon icon="mdi:file-outline" width="14" class="mr-1 shrink-0" />
						<span class="truncate font-mono text-xs">{entry.name}</span>
					</button>
				{/if}
			{/each}
		{/if}
	</div>
{/snippet}

<div class="mt-1 pl-1">
	{@render directory('/', '/', 0)}
</div>
