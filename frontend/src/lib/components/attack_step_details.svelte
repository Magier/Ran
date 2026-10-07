<script lang="ts">
	import type { AttackStep } from '#lib/api/index.js';
	import { actionDisplayName, displayArgumentValue } from '#lib/actionDisplay.js';
	import { getCampaignState } from './CampaignState.svelte';
	import Icon from '@iconify/svelte';
	import { Switch } from '@skeletonlabs/skeleton-svelte';
	import { tick } from 'svelte';

	interface ActionDetailProps {
		step: AttackStep;
	}

	let { step }: ActionDetailProps = $props();
	const campaignState = getCampaignState();
	const liveOutput = $derived(campaignState.getExecutionOutput(step?.id ?? ''));
	let status = $derived(
		liveOutput?.completed
			? liveOutput.success
				? 'Success'
				: 'Failed'
			: (step?.status ?? 'Unknown')
	);
	const badgeStyle = $derived.by(() => {
		switch (status) {
			case 'Success':
				return 'preset-filled-success-500';
			case 'Partial':
				return 'preset-filled-warning-500';
			case 'Failed':
				return 'preset-filled-error-500';
			case 'Ongoing':
				return 'preset-filled-warning-500';
			default:
				return 'preset-filled-default-500';
		}
	});
	const target = $derived(step?.targetId ? campaignState.getEntityById(step.targetId) : undefined);
	const stdout = $derived(liveOutput?.stdout ?? step?.stdout ?? step?.results?.[0] ?? '');
	const stderr = $derived(liveOutput?.stderr ?? step?.stderr ?? step?.results?.[1] ?? '');
	const outputTruncated = $derived(liveOutput?.truncated ?? step?.outputTruncated ?? false);
	const actionName = $derived(
		actionDisplayName(campaignState.getTtpById(step.TTP.id)?.title, step.TTP.name, step.args)
	);
	const parameters = $derived(
		Object.entries(step.args ?? {}).sort(([left], [right]) => left.localeCompare(right))
	);
	let followOutput = $state(true);
	let outputContainer: HTMLDivElement | undefined = $state();
	$effect(() => {
		void stdout;
		void stderr;
		if (followOutput && outputContainer) {
			void tick().then(() => {
				if (outputContainer) outputContainer.scrollTop = outputContainer.scrollHeight;
			});
		}
	});

	// JWTs always start with eyJ (base64url of '{"'). Replace with a short
	// placeholder so commands stay readable; the full token is kept in data-source.
	const JWT_RE = /eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]*/g;
	function redactJwt(cmd: string): string {
		return cmd.replace(JWT_RE, (tok) => `[jwt…${tok.slice(-6)}]`);
	}

	function handleCopy(event: MouseEvent) {
		const button = event.currentTarget as HTMLButtonElement;
		const codeEl = button.previousElementSibling as HTMLElement;
		if (codeEl) {
			// data-source holds the original (unredacted) text when set as a value
			const text = codeEl.dataset.source || codeEl.textContent?.trim() || '';
			if (text) {
				navigator.clipboard.writeText(text);
			}
		}
	}

	// --- Multi-hop traversal -------------------------------------------------
	// `traversal` is ordered outermost (C2 entry) → innermost (target). The chain
	// of systems is the first hop's source followed by every hop's destination,
	// so a command that pivots C2 → pod → node → target yields four nodes.
	const hops = $derived(step?.traversal ?? []);
	const hasTraversal = $derived(hops.length > 0);
	const chainNodes = $derived(hasTraversal ? [hops[0].fromId, ...hops.map((h) => h.toId)] : []);

	// Selected node in the chain. A node at index i < hops.length is the *source*
	// of hops[i] (it runs that hop's command); the final node is the target and
	// runs the bare inner command.
	let selectedNodeIdx = $state(0);
	$effect(() => {
		// Reset to the C2 entry whenever a different step is shown.
		void step?.id;
		selectedNodeIdx = 0;
	});
	const selectedHop = $derived(selectedNodeIdx < hops.length ? hops[selectedNodeIdx] : null);
	const selectedCommand = $derived(selectedHop ? selectedHop.command : (step?.innerCommand ?? ''));
	function extractEmbeddedCommand(envelope: string | undefined, command: string): string {
		if (!envelope) return '';

		const marker = '${CMD}';
		const markerStart = envelope.indexOf(marker);
		if (markerStart < 0) return '';

		const prefix = envelope.slice(0, markerStart);
		const suffix = envelope.slice(markerStart + marker.length);
		if (!command.startsWith(prefix) || !command.endsWith(suffix)) return '';

		return command.slice(prefix.length, command.length - suffix.length);
	}
	const embeddedCommand = $derived(
		selectedHop?.embeddedCommand ?? extractEmbeddedCommand(selectedHop?.envelope, selectedCommand)
	);
	const embeddedCommandStart = $derived(
		embeddedCommand ? selectedCommand.indexOf(embeddedCommand) : -1
	);

	// Trim an entity id down to a readable chip label, keeping a short type hint.
	function shortName(id: string): string {
		if (!id) return 'C2';
		if (id.startsWith('c2/')) return 'C2';
		const parts = id.split('/').filter(Boolean);
		return parts[parts.length - 1] || id;
	}
	function nodeLabel(id: string): string {
		if (id.startsWith('node/')) return `node ${shortName(id)}`;
		if (id.includes('/pod/')) return `pod ${shortName(id)}`;
		return shortName(id);
	}
</script>

{#if step != null}
	<header class="flex-none justify-between">
		<h4 class="h4">{actionName}</h4>
		<!-- {#if step.TTP.icon}
				<img src={step.TTP.icon} alt="TTP Icon" class="h-6 w-6" />
			{/if} -->
		<div class="mt-4 flex justify-start">
			<div class="pr-2">Tactic</div>
			<div class="badge">{step.TTP.tactic}</div>
		</div>
		{#if step.TTP.techniques?.length >= 1}
			<div class="flex justify-start">
				<div class="pr-2">Technique</div>
				<div class="badge">{step.TTP.techniques[0]}</div>
			</div>
		{/if}
		<p class="mt-2 opacity-60">
			{step.TTP.description}
		</p>

		<div class="mt-4 flex items-center justify-start">
			<div class="pr-2">Target:</div>
			<code class="inline text-base">{target?.name}</code>
		</div>
		{#if step.executedOn != target?.name}
			<div class="mt-4 flex items-center justify-start">
				<div class="pr-2">Executed On:</div>
				<code class="inline text-base">{step.executedOn}</code>
			</div>
		{/if}

		<div class="mt-4 flex items-center justify-start">
			<div class="pr-2">Started:</div>
			<code class="inline text-base">{step.startedAt}</code>
		</div>
		<div class=" flex items-center justify-start">
			<div class="pr-2">Completed:</div>
			<code class="inline text-base">{step.completedAt}</code>
		</div>
		<div class="mt-4 flex justify-start">
			<div class="pr-2">Status</div>
			<div class={['badge', badgeStyle]}>{status}</div>
		</div>
	</header>
	<article class="flex min-h-10 flex-auto flex-col overflow-auto">
		{#if parameters.length > 0}
			<details class="justify-start">
				<summary class="cursor-pointer pr-2">Parameters</summary>
				<dl class="bg-surface-200-800/40 divide-surface-300-700 mt-2 divide-y rounded px-3">
					{#each parameters as [name, value] (name)}
						<div class="grid grid-cols-[minmax(0,1fr)_minmax(0,2fr)] gap-3 py-2 text-sm">
							<dt class="truncate font-mono opacity-70" title={name}>{name}</dt>
							<dd class="min-w-0 font-mono break-all">{displayArgumentValue(name, value)}</dd>
						</div>
					{/each}
				</dl>
			</details>
		{/if}
		{#if step.reasoning?.trim()}
			<details class={['justify-start', parameters.length > 0 ? 'mt-1' : 'mt-4']}>
				<summary class="cursor-pointer pr-2">Reasoning</summary>
				<p class="mt-2 text-sm whitespace-pre-wrap opacity-80">{step.reasoning}</p>
			</details>
		{/if}
		<div class="mt-4 justify-start">
			{#if hasTraversal}
				<div class="mb-2 pr-2">Traversal</div>
				{#each step.routeWarnings as warning, index (index)}
					<div
						class="bg-warning-100-900 text-warning-700-300 mb-2 flex items-start gap-2 rounded p-2 text-xs"
					>
						<Icon icon="mdi:alert-outline" width="16" class="mt-px shrink-0" />
						<span>{warning.message}</span>
					</div>
				{/each}
				<!-- System chain: click a system to inspect the command + envelope at that hop -->
				<div class="flex min-h-8 flex-wrap items-center gap-y-1">
					{#each chainNodes as node, i (i)}
						{#if selectedHop && i === selectedNodeIdx}
							{#if i > 0}
								<Icon icon="material-symbols:chevron-right" width="16" class="opacity-40" />
							{/if}
							<div
								class="bg-surface-300-700/80 flex items-center rounded-md p-1"
								role="group"
								aria-label={`Selected hop from ${shortName(node)} to ${shortName(chainNodes[i + 1])}`}
							>
								<button
									type="button"
									class="bg-surface-400-600 max-w-full truncate rounded px-2 py-1 text-xs"
									onclick={() => (selectedNodeIdx = i)}
									title={node || 'C2'}>{nodeLabel(node)}</button
								>
								<Icon icon="material-symbols:arrow-forward" width="16" class="mx-0.5 opacity-50" />
								<button
									type="button"
									class="bg-surface-100-900/60 hover:bg-surface-100-900 max-w-full truncate rounded px-2 py-1 text-xs transition-colors"
									onclick={() => (selectedNodeIdx = i + 1)}
									title={chainNodes[i + 1] || 'C2'}>{nodeLabel(chainNodes[i + 1])}</button
								>
							</div>
						{:else if !(selectedHop && i === selectedNodeIdx + 1)}
							{#if i > 0}
								<Icon icon="material-symbols:chevron-right" width="16" class="opacity-40" />
							{/if}
							<button
								type="button"
								class={[
									'max-w-full truncate rounded px-2 py-1 text-xs transition-colors',
									selectedNodeIdx === i
										? 'bg-surface-400-600'
										: 'bg-surface-200-800/50 hover:bg-surface-200-800'
								]}
								onclick={() => (selectedNodeIdx = i)}
								title={node || 'C2'}>{nodeLabel(node)}</button
							>
						{/if}
					{/each}
				</div>

				<!-- Detail for the selected hop (or the target's inner command) -->
				<div class="bg-surface-100-900 mt-2 space-y-2 rounded p-2">
					<div>
						<div class="label mb-0.5 flex items-center gap-2 text-xs opacity-60">
							<span>{selectedHop ? 'Command sent over this hop' : 'Command on target'}</span>
							{#if selectedHop}
								<span class="badge preset-filled-surface-500 text-xs">{selectedHop.relation}</span>
							{/if}
						</div>
						<div class="bg-surface-50-950 group relative">
							<code
								class="block w-full overflow-x-hidden overflow-y-auto text-sm break-all whitespace-pre-wrap"
								data-source={selectedCommand}
								>{#if embeddedCommandStart >= 0}{redactJwt(
										selectedCommand.slice(0, embeddedCommandStart)
									)}<span
										class="bg-primary-500/30 text-primary-400 rounded px-0.5"
										title="Nested command data">{redactJwt(embeddedCommand)}</span
									>{redactJwt(
										selectedCommand.slice(embeddedCommandStart + embeddedCommand.length)
									)}{:else}{redactJwt(selectedCommand)}{/if}</code
							>
							<button
								class="btn bg-surface-200-800/40 hover:bg-surface-200-800/70 absolute top-1 right-1 px-1 py-0.5 opacity-0 transition-opacity group-hover:opacity-90"
								data-trigger
								onclick={handleCopy}
								><Icon icon="material-symbols:content-copy" width="16" /></button
							>
						</div>
					</div>
				</div>
			{:else}
				<div class="pr-2">Command</div>
				<div class="bg-surface-50-950 group relative">
					<code
						class="h-10 w-full overflow-x-hidden overflow-y-auto text-base break-all whitespace-pre-wrap"
						data-source={step.command}>{redactJwt(step.command)}</code
					>
					<button
						class="btn bg-surface-200-800/40 hover:bg-surface-200-800/70 absolute top-1 right-1 px-1 py-0.5 opacity-0 transition-opacity group-hover:opacity-90"
						data-trigger
						onclick={handleCopy}><Icon icon="material-symbols:content-copy" width="16" /></button
					>
				</div>
			{/if}
		</div>
		<div class="mt-4 w-full space-y-3">
			<div class="flex flex-wrap items-center gap-2">
				<span class="label flex-none">Output</span>
				<div class="flex flex-wrap items-center gap-2">
					{#if status === 'Ongoing' && (stdout || stderr)}
						<Switch
							checked={followOutput}
							onCheckedChange={(details) => (followOutput = details.checked)}
							class="flex items-center gap-1 text-xs"
						>
							<Switch.Control><Switch.Thumb /></Switch.Control>
							<Switch.Label>Follow output</Switch.Label>
							<Switch.HiddenInput />
						</Switch>
					{/if}
				</div>
			</div>
			{#if outputTruncated}
				<p class="text-warning-500 text-xs">Earlier live output was omitted from this view.</p>
			{/if}
			{#if !stdout && !stderr && status === 'Ongoing'}
				<p class="text-surface-500 flex items-center gap-1 text-sm">
					<Icon icon="svg-spinners:90-ring-with-bg" class="size-3" />
					Waiting for output…
				</p>
			{:else if !stdout && !stderr}
				<p class="text-surface-500 text-sm">Completed without output.</p>
			{/if}
			<div bind:this={outputContainer} class="max-h-[32rem] space-y-3 overflow-y-auto">
				{#if stdout}
					<div>
						<div class="text-surface-500 mb-1 text-xs">stdout</div>
						<div class="bg-surface-50-950 group relative">
							<code
								class="w-full overflow-x-hidden overflow-y-auto text-sm break-all whitespace-pre-wrap"
								data-source={stdout}
							>
								{stdout}
							</code>
							<button
								class="btn bg-surface-200-800/40 hover:bg-surface-200-800/70 absolute top-1 right-1 px-1 py-0.5 opacity-0 transition-opacity group-hover:opacity-90"
								data-trigger
								onclick={handleCopy}
								><Icon icon="material-symbols:content-copy" width="16" /></button
							>
						</div>
					</div>
				{/if}
				{#if stderr}
					<div>
						<div class="text-warning-500 mb-1 text-xs">stderr</div>
						<div class="bg-surface-50-950 group relative">
							<code class="block text-sm break-all whitespace-pre-wrap" data-source={stderr}
								>{stderr}</code
							>
							<button
								class="btn bg-surface-200-800/40 hover:bg-surface-200-800/70 absolute top-1 right-1 px-1 py-0.5 opacity-0 transition-opacity group-hover:opacity-90"
								data-trigger
								onclick={handleCopy}
								><Icon icon="material-symbols:content-copy" width="16" /></button
							>
						</div>
					</div>
				{/if}
			</div>
		</div>
	</article>
	<footer class="flex-none"></footer>
{/if}

<style>
	code {
		overflow-wrap: anywhere;
	}
</style>
