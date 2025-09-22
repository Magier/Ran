<script lang="ts">
	import { Combobox } from '@skeletonlabs/skeleton-svelte';

	import { parseEntityId, type Param } from '$lib/model';
	import type { domain } from '$lib/wailsjs/go/models';
	import { getCampaignState, type Entity } from '$lib/components/CampaignState.svelte';
	import { onMount } from 'svelte';

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
	}

	interface Constraint {
		Namespace?: string;
		Pod?: string;
		Entitlement?: string;
	}

	const campaignState = getCampaignState();

	let procedureId = $state(ttp.procedures?.[0]?.Key || '');
	let args = $state<Arg[]>([]);
	let availableEntities: Entity[] = $state([]);
	let namespaceArgName: string = "";

	let argOptions: Record<string, ComboboxOption[]> = $state({});

	let selectedNamespace = $derived.by(() => {
		const nsArg = args.find(arg => arg.Type === 'Namespace');
		return nsArg ? nsArg.Value : '';
	});
	const isSetTargetTTP =  ttp.id === 'use-kubeconfig'; // special handling for the setTarget TTP

	onMount(() => {
		args = ttp.params?.map((param: Param) => {
				let value = param.Default;
				if (argContext && param.Name in argContext) {
					value = argContext[param.Name];
				}

				if (value === '${TARGET}') {
					value = targetId;
					if (param.Type === 'string') { 
						// if the type is string, then only the name of ther target is relevant
						const e = parseEntityId(targetId);
						value = e?.name || '';
					}
					console.log("Setting target", param.Name, "to value", value);
				}
				if (param.Type === 'Namespace') {
					namespaceArgName = param.Name;
					if (param.Default === "" && targetId.startsWith("ns/")) {
						value = targetId.split("/")[1];
					}
				} else if (param.Type === 'Pod') {
					availableEntities = campaignState.getPods("", isSetTargetTTP)
					argOptions[param.Name] = availableEntities.map(entityToComboboxOption);
				} else if (param.Type === 'ServiceAccount') {
					availableEntities = campaignState.getServiceAccounts(selectedNamespace);
					argOptions[param.Name] = availableEntities.map(entityToComboboxOption);
				}

				return {
					Name: param.Name,
					Value: value,
					IsTrue: param.Default === 'true',
					Description: param.Description,
					Type: param.Type,
					Required: param.Required
				};
			}) || [];
	});

	// namespace options
	$effect(() => {
		const uniqueNamespaces = availableEntities.reduce((nss: Set<string>, r) => {
			if (r.namespace) { nss.add(r.namespace); }
			return nss;
		}, new Set<string>());
		argOptions[namespaceArgName] = Array.from(uniqueNamespaces.values()).map(ns => ({ label: ns, value: ns }));
	});

	function entityToComboboxOption(e: Entity): ComboboxOption {
		return { label: e.name, value: e.id, group: e.namespace };
	}

	// the args will be the final arguments used when executing the TTP
	function onInternalExecute() {
		const argsDict = args.reduce(
			(acc: { [key: string]: string }, arg) => {
				const isTemplateVar = arg.Value.startsWith("${") && arg.Value.endsWith("}");
				if (isTemplateVar) {
					// if the value is a variable, do not do any conversion
				} else if (arg.Type === 'bool') {
					arg.Value = arg.IsTrue ? 'true' : 'false';
				} else if (arg.Type === 'int') {
					let v = parseInt(arg.Value);
					// temporary workaround: accept strings as well, if parsing fails, maybe backend can recover
					if (!isNaN(v)) { 
						arg.Value = v.toString();
					}
				} else if (arg.Type === 'float') {
					let v = parseFloat(arg.Value);
					// temporary workaround: accept strings as well, if parsing fails, maybe backend can recover
					if (!isNaN(v)) { 
						arg.Value = v.toString();
					}
					// arg.Value = parseFloat(arg.Value).toString();
				} else if (arg.Type === 'string') {
					arg.Value = arg.Value.toString();
				} else {
					// for non-primitive types ensure the value is in the expected format,
					// i.e. if the arg is a name, it should not be a full id
					if (arg.Name.toLowerCase().endsWith("name") && arg.Value.indexOf("/") !== -1) {
						const parts = arg.Value.split("/");
						// the actual name is the last part of the id
						arg.Value = parts[parts.length - 1];
					}
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

	function getArgOptions(argName: string): ComboboxOption[] {
		let opts = argOptions[argName] ?? [];
		if (selectedNamespace !== "" && argName !== namespaceArgName) {
			opts = opts.filter(o => o.group === selectedNamespace);
		}
		return opts
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

			<label class="h5 label mt-5" for="procedure">Procedure</label>
			{#if ttp.procedures && ttp.procedures.length > 1}
				<select id="procedure" class="input mt-2" bind:value={procedureId} disabled={ttp.procedures.length <= 1}>
					{#each ttp.procedures as procedure}
						<option
							value={procedure.Key}
							disabled={executingSystemHasTool(targetId, procedure.Key)}
							>{procedure.Key}
						</option>
					{/each}
				</select>
			{:else}
				<code id="procedure" class="label mt-2">{procedureId}</code>
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
							{:else if getArgOptions(arg.Name).length > 0}
								<Combobox
									data={getArgOptions(arg.Name)}
									onValueChange={(e) => {
										arg.Value = e.value[0]
										console.info(`Selected value for ${arg.Name}: ${arg.Value}`);
									}}
									inputBehavior="autocomplete"
									allowCustomValue={true}
									defaultValue={[arg.Value]}
									placeholder={arg.Name + "..."}
									>
									<!-- This is optional. Combobox will render label by default -->
									{#snippet item(item)}
										<div class="flex w-full justify-between space-x-2">
											<span
												style="max-width: 15em; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; display: inline-block;"
												title={item.label}
											>
												{item.label}
											</span>
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
			{/if}
	</article>
	<footer class="flex justify-end gap-4">
		<button type="button" class="btn preset-tonal" onclick={onCancel}>Cancel</button>
		<button type="submit" class="btn preset-filled">Execute</button>
	</footer>
</form>
