<script lang="ts">
	import { Combobox } from '@skeletonlabs/skeleton-svelte';

	import { parseEntityId, type Param } from '$lib/model';
	import { GetRunningPods } from '$lib/wailsjs/go/main/App';
	import type { domain, main } from '$lib/wailsjs/go/models';
	import { getCampaignState } from '$lib/components/CampaignState.svelte';

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
		Required?: boolean;
		Options?: ComboboxOption[];
	}

	interface Constraint {
		Namespace?: string;
		Pod?: string;
		Entitlement?: string;
	}

	const campaignState = getCampaignState();

	let procedureId = $state(ttp.procedures?.[0]?.Key || '');
	let args = $state<Arg[]>([]);
	let availablePods: ComboboxOption[] = $state([]);

	let selectedNamespace = $derived.by(() => {
		const nsArg = args.find(arg => arg.Type === 'Namespace');
		return nsArg ? nsArg.Value : '';
	});
	// namespace options
	$effect(() => {
		let nsArg = args.find(arg => arg.Name === 'Namespace');
		if (nsArg) {
			nsArg.Options = availablePods
				.map(pod => pod.group)
				.filter((ns): ns is string => typeof ns === 'string')
				.filter((value, index, self) => self.indexOf(value) === index)
				.map(ns => ({ label: ns, value: ns }));
		}
	});

	// pod options
	$effect(() => {
		let resArg = args.find(arg => arg.Type === 'Pod');
		if (resArg) {
			resArg.Options = availablePods.filter(pod => selectedNamespace ? pod.group === selectedNamespace : true);
		}
	});

	// special handling for the InitialAccess technique, because it may use information not yet inferred during the campaign
	$effect(() => {
		if (procedureId === 'setTarget') {
			GetRunningPods("").then(pods => {
				availablePods = pods.map((pod: main.K8sResource) => ({ label: pod.name, value: pod.id, group: pod.namespace }));
			});	
		}
	})

	// the args will be the final arguments used when executing the TTP
	$effect(() => {
		args = ttp.params?.map((param: Param) => {
				let value = param.Default;
				let options: ComboboxOption[] | undefined = undefined;
				if (argContext && param.Name in argContext) {
					value = argContext[param.Name];
				}

				if (value === '${TARGET}') {
					let id = parseEntityId(targetId);
					value = id.name;
				}
				if (param.Type === 'Namespace') {
					if (param.Default === "" && targetId.startsWith("ns/")) {
						value = targetId.split("/")[1];
						console.log("set default NS to ", value)
					}
					const namespaces = campaignState.getNamespaces();
					options = namespaces.map(ns => ({ label: ns.name, value: ns.name }));
				} else if (param.Type === 'Pod') {
					options = campaignState.getPods(selectedNamespace).map(pod => ({
						label: pod.name,
						value: pod.id,
						group: pod.namespace
					}));
				} else if (param.Type === 'ServiceAccount') {
					const serviceAccounts = campaignState.getServiceAccounts(selectedNamespace);
					// TODO filter by entitlements (if known)
					let saOptions = serviceAccounts.map(sa => {
						const saId = sa.id || `ns/${sa.namespace}/sa/${sa.name}`;
						return { label: sa.name, value: saId}
					 });
					 console.log("sa options ", saOptions)
					options = saOptions;
				}

				return {
					Name: param.Name,
					Value: value,
					IsTrue: param.Default === 'true',
					Description: param.Description,
					Type: param.Type,
					Options: options,
					Required: param.Required
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

<form class="text-surface-50 w-full space-y-8" onsubmit={onInternalExecute}>
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
								<Combobox
									data={arg.Options}
									value={[arg.Value]}
									onValueChange={(e) => (arg.Value = e.value[0])}
									placeholder={arg.Name + "..."}
									>
									<!-- This is optional. Combobox will render label by default -->
									{#snippet item(item)}
										<div class="flex w-full justify-between space-x-2">
											<span>{item.label}</span>
											<span>{item.group}</span>
										</div>
									{/snippet}
								</Combobox>
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
		<button type="submit" class="btn preset-filled">Execute</button>
	</footer>
</form>
