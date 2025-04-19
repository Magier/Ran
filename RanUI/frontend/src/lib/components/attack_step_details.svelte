<script lang="ts">
	import type { TTP } from '$lib/model';
	import type { campaign } from '$lib/wailsjs/go/models';
	import { derived } from 'svelte/store';

	interface ActionDetailProps {
		step: campaign.AttackStep;
		icon?: any;
		// conditions?: Object;
		// onclick: (ttp: TTP) => void;
	}

	let { step }: ActionDetailProps = $props();
	// let badgeStyle = $derived.by(() => {
	// 	step ? 'preset-filled-success-500' : 'preset-filled-error-500';
	// });
	const badgeStyle = step?.Success ? 'preset-filled-success-500' : 'preset-filled-error-500';
	let status = step == null ? 'unknown' : step.Success ? 'Success' : 'Failed';
</script>

{#if step != null}
	<header class="flex justify-between">
		<h4 class="h4">{step.TTP.name}</h4>
	</header>
	<article>
		<p class="opacity-60">
			{step.TTP.description}
		</p>

		<div class="mt-4 flex justify-start">
			<div class="pr-2">Status</div>
			<div class={['badge', badgeStyle]}>{status}</div>
		</div>

		<div class="mt-4">
			<span class="label">Result</span>
			{#each step.Results as result}
				<code class="">{result}</code>
			{/each}
		</div>
	</article>
	<footer></footer>
{/if}
