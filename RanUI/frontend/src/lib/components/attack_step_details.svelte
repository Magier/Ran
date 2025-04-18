<script lang="ts">
	import type { TTP } from '$lib/model';

	interface ActionDetailProps {
		ttp: TTP;
		icon: any;
		status: string;
		result: string;
		// conditions?: Object;
		// onclick: (ttp: TTP) => void;
	}

	let { ttp, status = 'Success', result = '-' }: ActionDetailProps = $props();

	$effect(() => {
		console.log('TTP:', ttp);
	});
</script>

{#if ttp != null}
	<header class="flex justify-between">
		<h4 class="h4">{ttp.name}</h4>
	</header>
	<article>
		<p class="opacity-60">
			{ttp.description}
		</p>

		<div class="mt-4">
			<span class="label">Status</span>
			<span class="badge preset-filled-success-500">{status}</span>
		</div>

		<div class="mt-4">
			<span class="label">Result</span>
			<code class="">{result}</code>
		</div>
	</article>
	<footer></footer>
{/if}
