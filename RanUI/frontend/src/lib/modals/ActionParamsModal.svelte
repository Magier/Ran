<script lang="ts">
	import type { TTP } from '$lib/model';
	import { onMount } from 'svelte';
	import Page from '../../routes/+page.svelte';

	interface ParamProps {
		ttp: TTP;
		onExecute: (ttp: TTP, args: Arg[]) => void;
		onCancel: () => void;
	}
	let { ttp, onExecute, onCancel }: ParamProps = $props();

	interface Arg {
		Name: string;
		Value: string;
		Description: string;
		Type: string;
	}

	let args: Arg[] = $state([]);

	$effect(() => {
		console.log('updating the TTP for the modal');
		args =
			ttp.params?.map((param) => {
				return {
					Name: param.Name,
					Value: param.Default,
					Description: param.Description,
					Type: 'text'
				};
			}) || [];
	});

	// $inspect(args);

	function onInternalExecute() {
		onExecute(ttp, $state.snapshot(args));
	}
</script>

<form class="w-full space-y-8">
	<header class="flex justify-between">
		<h4 class="h4">{ttp.name}</h4>
	</header>
	<article>
		<div class="">
			<span class="h5 label">Description</span>
			{ttp.description}
		</div>
		<div class="">
			<label class="label mt-5">
				<span class="label-text">Target</span>
				<input
					class="input"
					type="text"
					bind:value={ttp.target}
					placeholder="Enter target IP or URL"
				/>
			</label>
			{#if args.length > 0}
				<fieldset class="mt-5">
					<span class="h5">Params</span>
					<div class="text-center">{args[0].Value}</div>
					<!-- note: value must be accessed directly via exploitParams, otherwise 2way binding won't work -->
					{#each args as arg}
						<div class="input-group mt-2 grid-cols-[auto_1fr_auto]">
							<div class="ig-cell preset-tonal">{arg.Name}</div>
							<input
								class="ig-input"
								bind:value={arg.Value}
								type={arg.Type}
								placeholder={arg.Description}
							/>
						</div>
					{/each}
				</fieldset>
			{/if}
		</div>
	</article>
	<footer class="flex justify-end gap-4">
		<button type="button" class="btn preset-tonal" onclick={onCancel}>Cancel</button>
		<button type="button" class="btn preset-filled" onclick={onInternalExecute}>Execute</button>
	</footer>
</form>
