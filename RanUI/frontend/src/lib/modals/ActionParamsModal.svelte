<script lang="ts">
	import type { Param, TTP } from '$lib/model';

	interface ParamProps {
		targetId: string;
		ttp: TTP;
		onExecute: (ttpId: string, args: Record<string, string>) => void;
		onCancel: () => void;
	}
	let { targetId = $bindable(), ttp, onExecute, onCancel }: ParamProps = $props();

	interface Arg {
		Name: string;
		Value: string;
		Description: string;
		Type: string;
		IsTrue: boolean;
	}

	// the args will be the final arguments used when executing the TTP
	let args: Arg[] = $derived.by(() => {
		return (
			ttp.params?.map((param: Param) => {
				return {
					Name: param.Name,
					Value: param.Default,
					IsTrue: param.Default === 'true',
					Description: param.Description,
					Type: param.Type
				};
			}) || []
		);
	});

	function onInternalExecute() {
		const argsDict = args.reduce(
			(acc: { [key: string]: string }, arg) => {
				acc[arg.Name] = arg.Value;
				return acc;
			},
			{} as { [key: string]: string }
		);

		onExecute(ttp.id, argsDict);
	}
</script>

<form class="text-surface-50 w-full space-y-8">
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
					bind:value={targetId}
					placeholder="Enter target IP or URL"
				/>
			</label>
			{#if args.length > 0}
				<fieldset class="mt-5">
					<span class="h5">Params</span>
					{#each args as arg}
						<div class="input-group mt-2 grid-cols-[auto_1fr_auto]">
							<div class="ig-cell preset-tonal">{arg.Name}</div>
							{#if arg.Type === 'bool'}
								<input
									class="ig-input"
									bind:checked={arg.IsTrue}
									type="checkbox"
									placeholder={arg.Description}
								/>
							{:else}
								<input
									class="ig-input"
									bind:value={arg.Value}
									type="text"
									placeholder={arg.Description}
								/>
							{/if}
							<!-- <input
								class="ig-input"
								bind:value={arg.Value}
								type={arg.Type}
								placeholder={arg.Description}
							/> -->
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
