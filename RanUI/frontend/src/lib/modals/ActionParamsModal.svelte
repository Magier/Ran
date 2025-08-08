<script lang="ts">
	import { parseEntityId, type Param } from '$lib/model';
	import { GetRunningPods } from '$lib/wailsjs/go/main/App';
	import type { domain, main } from '$lib/wailsjs/go/models';

	interface ParamProps {
		targetId: string;
		ttp: domain.TTP;
		argContext: Record<string, any>;
		onExecute: (ttpId: string, procedureId: string, args: Record<string, string>) => void;
		onCancel: () => void;
	}
	let { targetId = $bindable(), ttp, argContext, onExecute, onCancel }: ParamProps = $props();

	interface ComboboxOption {
		label: string;
		value: string;
		group?: string;
	}
	interface Arg {
		Name: string;
		Value: string;
		Description: string;
		Type: string;
		IsTrue: boolean;
		Options?: ComboboxOption[];
	}


	let procedureId = $state(ttp.procedures?.[0]?.Key || '');
	let args = $state<Arg[]>([]);
	let availablePods: ComboboxOption[] = [];
	// the args will be the final arguments used when executing the TTP
	$effect(() => {
		if (procedureId === 'setTarget') {
			GetRunningPods("").then(pods => {
				availablePods = pods.map((pod: main.K8sResource) => ({ label: pod.name, value: pod.id, group: pod.namespace }));

				let targetArg = args.find(arg => arg.Name === 'TargetName');
				if (targetArg) {
					targetArg.Options = availablePods
				} else {
					console.warn("targetArg not found")
				}
				let nsArg = args.find(arg => arg.Name === 'Namespace');
				if (nsArg) {
					nsArg.Options = availablePods.map(pod => pod.group).filter((value, index, self) => self.indexOf(value) === index).map(ns => ({ label: ns, value: ns }));
				} else {
					console.warn("ns not found")
				}
			});	
		}

		args =
			ttp.params?.map((param: Param) => {
				let value = param.Default;
				if (argContext && param.Name in argContext) {
					value = argContext[param.Name];
				}

				if (value === '${TARGET}') {
					let id = parseEntityId(targetId);
					value = id.name;
				}

				return {
					Name: param.Name,
					Value: value,
					IsTrue: param.Default === 'true',
					Description: param.Description,
					Type: param.Type,
				};
			}) || [];
	});

	function onInternalExecute() {
		const argsDict = args.reduce(
			(acc: { [key: string]: string }, arg) => {
				if (arg.Type === 'bool') {
					arg.Value = arg.IsTrue ? 'true' : 'false';
				} else if (arg.Type === 'int') {
					arg.Value = parseInt(arg.Value).toString();
				} else if (arg.Type === 'float') {
					arg.Value = parseFloat(arg.Value).toString();
				} else if (arg.Type === 'string') {
					arg.Value = arg.Value.toString();
				}
				acc[arg.Name] = arg.Value;
				return acc;
			},
			{} as { [key: string]: string }
		);

		onExecute(ttp.id, procedureId, argsDict);
	}

	function executingSystemHasTool(system: string, tool: string): boolean {
		console.warn('Checking of available tools is not implemented yet.');
		return false;
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
			<span class="h5 label mt-5">Procedure</span>
			{#if ttp.procedures && ttp.procedures.length > 1}
				<ul class="list-disc pl-5">
					<select class="input mt-2" bind:value={procedureId} disabled={ttp.procedures.length <= 1}>
						{#each ttp.procedures as procedure}
							<option
								value={procedure.Key}
								disabled={executingSystemHasTool(targetId, procedure.Key)}
								>{procedure.Key}
							</option>
						{/each}
					</select>
				</ul>
			{:else}
				<code class="label mt-2">{procedureId}</code>
			{/if}
			<!-- <label class="label mt-5">
				<span class="label-text">Target</span>
				<input
					class="input"
					type="text"
					bind:value={targetId}
					placeholder="Enter target IP or URL"
				/>
			</label> -->
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
							{:else if arg.Options}
								<select class="select" bind:value={arg.Value}>
									{#each arg.Options as option}
										<option value={option.value}>{option.label}</option>
									{/each}
								</select>
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
