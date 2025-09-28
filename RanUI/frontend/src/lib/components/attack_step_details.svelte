<script lang="ts">
	import type { campaign } from '$lib/wailsjs/go/models';
	import ObservableInfo from './observable_info.svelte';

	interface ActionDetailProps {
		step: campaign.AttackStep;
		icon?: any;
	}

	let { step }: ActionDetailProps = $props();
	const badgeStyle = step?.Success ? 'preset-filled-success-500' : 'preset-filled-error-500';
	let status = step == null ? 'unknown' : step.Success ? 'Success' : 'Failed';

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

		<div class="mt-4 flex justify-start">
			<div class="pr-2">Target:</div>
			<code>{step.Target?.name}</code>
		</div>
		{#if step.ExecutedOn?.name != step.Target?.name }
		<div class="mt-4 flex justify-start">
			<div class="pr-2">Executed On:</div>
			<code>{step.ExecutedOn?.name}</code>
		</div>
		{/if}

		<div class="mt-4 flex justify-start">
			<div class="pr-2">Started</div>
			<div class="badge">{step.StartAt}</div>
		</div>
		<div class=" flex justify-start">
			<div class="pr-2">Completed</div>
			<div class="badge">{step.CompletedAt}</div>
		</div>
		<div class="mt-4 flex justify-start">
			<div class="pr-2">Status</div>
			<div class={['badge', badgeStyle]}>{status}</div>
		</div>
	</header>
	<article class="flex min-h-10 flex-auto flex-col overflow-auto">
		<div class="mt-4 justify-start">
			<div class="pr-2">Command</div>
			<code class="h-10 w-full overflow-y-auto overflow-x-hidden whitespace-pre-wrap break-all"
				>{step.Command}</code
			>
		</div>
		<div class="mt-4 w-full">
			<span class="label mb-1 flex-none">Result:</span>
			{#each step.Results as result}
				{#if result}
				<div class="bg-surface-50-950 relative group">
					<code class="w-full overflow-y-auto overflow-x-hidden whitespace-pre-wrap break-all" data-source>
						{result}
					</code>
					<button
						class="btn preset-filled absolute top-1 right-1 opacity-0 group-hover:opacity-90 transition-opacity"
						data-trigger
						onclick={handleCopy}
					>📋</button>
				</div>
				{/if}
			{/each}
		</div>

		<div class="mt-4 w-full">
			<span class="label mb-1 flex-none">Observables:</span>
		{#if step.Observables?.length > 0}
			<div class="mt-4 w-full">
				{#each step.Observables as obs}
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
