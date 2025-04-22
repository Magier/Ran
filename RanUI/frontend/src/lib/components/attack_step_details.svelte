<script lang="ts">
	import type { campaign } from '$lib/wailsjs/go/models';

	interface ActionDetailProps {
		step: campaign.AttackStep;
		icon?: any;
	}

	let { step }: ActionDetailProps = $props();
	const badgeStyle = step?.Success ? 'preset-filled-success-500' : 'preset-filled-error-500';
	let status = step == null ? 'unknown' : step.Success ? 'Success' : 'Failed';
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
			<div class="pr-2">Target</div>
			<code>{step.Target?.Name}</code>
		</div>

		<div class="mt-4 flex justify-start">
			<div class="pr-2">Started</div>
			<div class="badge">{step.StartAt}</div>
		</div>
		<div class=" flex justify-start">
			<div class="pr-2">Completed</div>
			<div class="badge">{step.CompletedAt}</div>
		</div>
		<div class="mt-4 justify-start">
			<div class="pr-2">Command</div>
			<code class="h-10 w-full overflow-y-auto overflow-x-hidden whitespace-pre-wrap break-all"
				>{step.Command}</code
			>
		</div>
		<div class="mt-4 flex justify-start">
			<div class="pr-2">Status</div>
			<div class={['badge', badgeStyle]}>{status}</div>
		</div>
	</header>
	<article class="flex min-h-10 flex-auto flex-col overflow-auto">
		<div class="mt-4 w-full">
			<span class="label mb-1 flex-none">Result:</span>
			{#each step.Results as result}
				<div class="bg-surface-50-950">
					<code class="w-full overflow-y-auto overflow-x-hidden whitespace-pre-wrap break-all"
						>{result}
					</code>
				</div>
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
