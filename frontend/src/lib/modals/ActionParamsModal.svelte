<script lang="ts">
	import { Combobox, Portal, type ComboboxRootProps, useListCollection, type ListCollection } from '@skeletonlabs/skeleton-svelte';

	import { parseEntityId, type Param } from '$lib/model';
	import type { domain } from '$lib/domain/models';
	import { getCampaignState, type Entity } from '$lib/components/CampaignState.svelte';
	import { untrack } from 'svelte';

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

	let procedureId = $state('');
	let args = $state<Arg[]>([]);
	let availableEntities: Entity[] = $state([]);
	let namespaceArgName: string = "";

	let argOptions: Record<string, ComboboxOption[]> = $state({});

	// Track previous TTP ID to detect when TTP changes
	let previousTtpId: string | undefined = undefined;

	let selectedNamespace = $derived.by(() => {
		const nsArg = args.find(arg => arg.Type === 'Namespace');
		return nsArg ? nsArg.Value : '';
	});

	function selectNamespace(ns: string) {
		// Update args immutably so Svelte's reactivity picks up the change
		args = args.map(a => a.Type === 'Namespace' ? { ...a, Value: ns } : a);
	}


	$effect(() => {
		// if the namespace changes, and there is a namespace argument, set it as well
		const outOfNsResources = args.find(arg => arg.Value.startsWith("ns/") && !arg.Value.startsWith(`ns/${selectedNamespace}`));
		console.info("selectedNS changed", outOfNsResources);

		if (outOfNsResources) {
			args = args.map(a =>
				a.Value.startsWith("ns/") && !a.Value.startsWith(`ns/${selectedNamespace}`)
					? { ...a, Value: "" }
					: a
			);
		}
	});

	// Initialize args when TTP changes
	$effect(() => {
		const currentTtpId = ttp?.id;
		
		// Only re-initialize if TTP actually changed
		if (currentTtpId === previousTtpId) {
			return;
		}
		previousTtpId = currentTtpId;
		
		// Capture all reactive values we need before untrack
		const ttpParams = ttp.params;
		const ttpTactic = ttp.tactic;
		const ttpProcedures = ttp?.procedures;
		const currentTargetId = targetId;
		const currentArgContext = argContext;
		
		// Derive initial namespace from targetId, not from selectedNamespace (which depends on args)
		const initialNamespace = currentTargetId.startsWith("ns/") ? currentTargetId.split("/")[1] : "";
		
		untrack(() => {
			console.group("ActionParamsModal: Initializing args for TTP", currentTtpId);
			
			// Reset argOptions when TTP changes
			argOptions = {};
			
			args = ttpParams?.map((param: Param) => {
					let value = param.Default;
					if (currentArgContext && param.Name in currentArgContext) {
						value = currentArgContext[param.Name];
					}

					if (value === '${TARGET}') {
						value = currentTargetId;
						if (param.Type === 'string') {
							// if the type is string, then only the name of ther target is relevant
							const e = parseEntityId(currentTargetId);
							value = e?.name || '';
						}
						console.log("Setting target", param.Name, "to value", value);
					}
					if (param.Type === 'Namespace') {
						namespaceArgName = param.Name;
						if (param.Default === "" && currentTargetId.startsWith("ns/")) {
							value = currentTargetId.split("/")[1];
						}
					} else if (param.Type === 'Pod') {
						const isSetTargetTTP = ttpTactic === 'Initial Access'; // special handling for the setTarget TTP, to use all available pods
						availableEntities = campaignState.getPods("", isSetTargetTTP)
						argOptions[param.Name] = availableEntities.map(entityToComboboxOption);
					} else if (param.Type === 'ServiceAccount') {
						// Use initialNamespace derived from targetId, not selectedNamespace
						availableEntities = campaignState.getServiceAccounts(initialNamespace);
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

			// Also reset procedureId when TTP changes
			procedureId = ttpProcedures?.[0]?.Key || '';

			console.log(args);
			console.groupEnd();
		});
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

	function getArgOptions(argName: string): ListCollection<ComboboxOption> {
		let opts = argOptions[argName] ?? [];
		if (selectedNamespace !== "" && argName !== namespaceArgName) {
			opts = opts.filter(o => o.group === selectedNamespace);
		}
		return opts
	}
	
	function toComboBoxCollection(items: ComboboxOption[]): ListCollection<ComboboxOption> {
		return useListCollection({
		  items: items,
		  itemToString: (item) => item.label,
		  itemToValue: (item) => item.value,
		  groupBy: (item) => item.group || undefined
		})
	}

	function onArgChange(arg, e) {
		// If the chosen item carries a namespace in its group, set it
		if (arg.Type !== 'Namespace') {
			const ns = e.items?.[0]?.group;
			if (ns) {selectNamespace(ns);}
		}
		// IMMUTABLE UPDATE so Svelte sees it:
		const i = args.findIndex(a => a.Name === arg.Name);
		if (i !== -1) {
			const newValue = e.value[0];
			args = args.with(i, { ...args[i], Value: newValue });
		} else {
			console.warn("Could not find arg to update:", arg.Name);
		}

		// arg.Value = e.value[0]
		// if (e.items.length > 0 && e.items[0].group ) {
		// 	selectNamespace(e.items[0].group);
		// }
	}


	// const onvalueChange: ComboboxRootProps['onValueChange'] = (event) => {
	// 	const filtered = data.filter((item) => item.value.toLowerCase().includes(event.inputValue.toLowerCase()));
	// 	if (filtered.length > 0) {
	// 		items = filtered;
	// 	} else {
	// 		items = data;
	// 	}
	// }

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
					{#each ttp.procedures as procedure (procedure.Key)}
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
{#each args as arg (arg.Name)}
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
				value={[arg.Value]}
				collection={toComboBoxCollection(getArgOptions(arg.Name))}
				onValueChange={(e) => onArgChange(arg, e)}
				inputBehavior="autocomplete"
				allowCustomValue={true}
				openOnChange={true}
				defaultValue={[arg.Value]}
				placeholder={arg.Name + "..."}
				>
				<Combobox.Control>
					<Combobox.Input />
					<Combobox.Trigger />
				</Combobox.Control>
				<Combobox.Positioner>
					<Combobox.Content class="z-50">
						{#each getArgOptions(arg.Name) as item (item)}
							<Combobox.Item {item}>
								<Combobox.ItemText>{item.label}</Combobox.ItemText>
								<Combobox.ItemIndicator />
							</Combobox.Item>
						{/each}
					</Combobox.Content>
				</Combobox.Positioner>
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
