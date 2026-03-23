<script lang="ts">
	import type { AttackStep } from '$lib/api';
	import { getCampaignState } from './CampaignState.svelte';
	import ObservableInfo from './observable_info.svelte';
	import Icon from '@iconify/svelte';

	interface ActionDetailProps {
		step: AttackStep;
		icon?: any;
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
	const target = $derived(step?.targetId ? campaignState.getEntityById(step.targetId) : "?");

	function handleCopy(event: MouseEvent) {
		const button = event.currentTarget as HTMLButtonElement;
		const codeEl = button.previousElementSibling as HTMLElement;
		if (codeEl && codeEl.dataset.source !== undefined) {
			const text = codeEl.textContent?.trim() ?? '';
			if (text) {
				navigator.clipboard.writeText(text);
			}
		}
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
		{#if step.TTP.techniques.length >= 1}
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
			<code class="text-base inline">{target?.name}</code>
		</div>
		{#if step.executedOn != target?.name }
		<div class="mt-4 flex items-center justify-start">
			<div class="pr-2">Executed On:</div>
			<code class="text-base inline">{step.executedOn}</code>
		</div>
		{/if}

		<div class="mt-4 flex justify-start items-center">
			<div class="pr-2">Started: </div>
			<code class="text-base inline">{step.startedAt}</code>
		</div>
		<div class=" flex justify-start items-center">
			<div class="pr-2">Completed: </div>
			<code class="text-base inline">{step.completedAt}</code>
		</div>
		<div class="mt-4 flex justify-start">
			<div class="pr-2">Status</div>
			<div class={['badge', badgeStyle]}>{status}</div>
		</div>
	</header>
	<article class="flex min-h-10 flex-auto flex-col overflow-auto">
		<div class="mt-4 justify-start">
			<div class="pr-2">Command</div>
				<div class="bg-surface-50-950 relative group">
			<code class="h-10 w-full overflow-y-auto text-base overflow-x-hidden whitespace-pre-wrap break-all" data-source
				>{step.command}</code>
			<button
				class="btn absolute top-1 right-1 opacity-0 group-hover:opacity-90 transition-opacity px-1 py-0.5 bg-surface-200-800/40 hover:bg-surface-200-800/70"
				data-trigger
				onclick={handleCopy}
				><Icon icon="material-symbols:content-copy" width="16" /></button>
			</div>
		</div>
		<div class="mt-4 w-full">
			<span class="label mb-1 flex-none">Result:</span>
			{#each step.results as result}
				{#if result}
				<div class="bg-surface-50-950 relative group">
					<code class="w-full text-sm overflow-y-auto overflow-x-hidden whitespace-pre-wrap break-all" data-source>
						{result}
					</code>
					<button
						class="btn absolute top-1 right-1 opacity-0 group-hover:opacity-90 transition-opacity px-1 py-0.5 bg-surface-200-800/40 hover:bg-surface-200-800/70"
						data-trigger
						onclick={handleCopy}
					><Icon icon="material-symbols:content-copy" width="16" /></button>
				</div>
				{/if}
			{/each}
		</div>

		<div class="mt-4 w-full">
			<span class="label mb-1 flex-none">Observables:</span>
		{#if step.observables?.length > 0}
			<div class="mt-4 w-full">
				{#each step.observables as obs}
					{#if obs}
						<ObservableInfo observable={obs} />
					{/if}
				{/each}
			</div>
		{:else}
			<div class="opacity-60 italic">No observables generated</div>
		{/if}
		</div>

	</article>
	<footer class="flex-none"></footer>
{/if}

<style>
	code {
		overflow-wrap: anywhere;
	}
</style>
