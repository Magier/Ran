<script lang="ts">
	import type { AttackStep } from '$lib/api';
	import { getCampaignState } from './CampaignState.svelte';
	import Icon from '@iconify/svelte';

	interface ActionDetailProps {
		step: AttackStep;
	}

	let { step }: ActionDetailProps = $props();
	const badgeStyle = $derived.by(() => {
		switch (step?.status) {
			case 'Success':
				return 'preset-filled-success-500';
			case 'Failed':
				return 'preset-filled-error-500';
			case 'Ongoing':
				return 'preset-filled-warning-500';
			default:
				return 'preset-filled-default-500';
		}
	});
	let status = $derived(step?.status ?? 'Unknown');

	const campaignState = getCampaignState();
	const target = $derived(step?.targetId ? campaignState.getEntityById(step.targetId) : undefined);

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
		<h4 class="h4">{step.TTP.name}</h4>
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
		{#if step.routeReason}
			<div class="mt-4 justify-start">
				<div class="mb-1 pr-2">Route</div>
				<p class="text-xs opacity-80">{step.routeReason}</p>
			</div>
		{/if}
		<div class="mt-4 justify-start">
			{#if hasTraversal}
				<div class="mb-2 pr-2">Traversal</div>
				<!-- System chain: click a system to inspect the command + envelope at that hop -->
				<div class="flex flex-wrap items-center gap-y-1">
					{#each chainNodes as node, i}
						{#if i > 0}
							<Icon icon="material-symbols:chevron-right" width="16" class="opacity-40" />
						{/if}
						<button
							type="button"
							class={[
								'max-w-full truncate rounded px-2 py-1 text-xs transition-colors',
								selectedNodeIdx === i
									? 'preset-filled-primary-500'
									: 'bg-surface-200-800/50 hover:bg-surface-200-800'
							]}
							onclick={() => (selectedNodeIdx = i)}
							title={node || 'C2'}>{nodeLabel(node)}</button
						>
					{/each}
				</div>

				<!-- Detail for the selected hop (or the target's inner command) -->
				<div class="bg-surface-100-900 mt-2 space-y-2 rounded p-2">
					{#if selectedHop}
						<div class="flex flex-wrap items-center gap-2 text-sm">
							<span class="opacity-70">{shortName(selectedHop.fromId)}</span>
							<Icon icon="material-symbols:arrow-forward" width="14" class="opacity-50" />
							<span class="opacity-70">{shortName(selectedHop.toId)}</span>
							<span class="badge preset-filled-surface-500 text-xs">{selectedHop.relation}</span>
						</div>
						{#if selectedHop.envelope}
							<div>
								<div class="label mb-0.5 text-xs opacity-60">Envelope</div>
								<code class="block text-xs break-all whitespace-pre-wrap"
									>{#each selectedHop.envelope.split('${CMD}') as part, pi}{#if pi > 0}<span
												class="bg-primary-500/30 text-primary-400 mx-0.5 rounded px-1 font-semibold"
												>{'${CMD}'}</span
											>{/if}{redactJwt(part)}{/each}</code
								>
							</div>
						{/if}
					{:else}
						<div class="flex flex-wrap items-center gap-2 text-sm">
							<span class="badge preset-filled-success-500 text-xs">runs on target</span>
							<span class="opacity-70">{shortName(chainNodes[chainNodes.length - 1])}</span>
						</div>
					{/if}
					<div>
						<div class="label mb-0.5 text-xs opacity-60">
							{selectedHop ? 'Command sent over this hop' : 'Command on target'}
						</div>
						<div class="bg-surface-50-950 group relative">
							<code
								class="block w-full overflow-x-hidden overflow-y-auto text-sm break-all whitespace-pre-wrap"
								data-source={selectedCommand}>{redactJwt(selectedCommand)}</code
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
		<div class="mt-4 w-full">
			<span class="label mb-1 flex-none">Result:</span>
			{#each step.results as result}
				{#if result}
					<div class="bg-surface-50-950 group relative">
						<code
							class="w-full overflow-x-hidden overflow-y-auto text-sm break-all whitespace-pre-wrap"
							data-source
						>
							{result}
						</code>
						<button
							class="btn bg-surface-200-800/40 hover:bg-surface-200-800/70 absolute top-1 right-1 px-1 py-0.5 opacity-0 transition-opacity group-hover:opacity-90"
							data-trigger
							onclick={handleCopy}><Icon icon="material-symbols:content-copy" width="16" /></button
						>
					</div>
				{/if}
			{/each}
		</div>
	</article>
	<footer class="flex-none"></footer>
{/if}

<style>
	code {
		overflow-wrap: anywhere;
	}
</style>
