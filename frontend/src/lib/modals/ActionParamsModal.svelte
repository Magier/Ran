<script lang="ts">
	import { Combobox, Portal, type ComboboxRootProps, useListCollection, type ListCollection } from '@skeletonlabs/skeleton-svelte';

	import { parseEntityId } from '$lib/model';
	import type { TTP, TTPParam } from '$lib/api/index';
	import { getCampaignState, type Entity } from '$lib/components/CampaignState.svelte';
	import { untrack } from 'svelte';

	interface ParamProps {
		targetId: string;
		ttp: TTP;
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
	// Incremented when a combobox value is set programmatically (external update),
	// used as a {#key} to force remount so defaultValue is re-applied.
	let argExternalVersions: Record<string, number> = $state({});

	function bumpArgVersion(name: string) {
		argExternalVersions = { ...argExternalVersions, [name]: (argExternalVersions[name] ?? 0) + 1 };
	}

	// Track previous TTP ID to detect when TTP changes
	let previousTtpId: string | undefined = undefined;

	let selectedNamespace = $derived.by(() => {
		const nsArg = args.find(arg => arg.Type === 'Namespace');
		return nsArg ? nsArg.Value : '';
	});

	function selectNamespace(ns: string) {
		// Update args immutably so Svelte's reactivity picks up the change
		args = args.map(a => {
			if (a.Type === 'Namespace') {
				bumpArgVersion(a.Name);
				return { ...a, Value: ns };
			}
			return a;
		});
	}

	$effect(() => {
		// if the namespace changes, clear out-of-namespace resources (except TOKEN which can be cross-namespace)
		const outOfNsResources = args.find(arg => 
			arg.Name !== "TOKEN" && 
			arg.Value.startsWith("ns/") && 
			!arg.Value.startsWith(`ns/${selectedNamespace}`)
		);

		if (outOfNsResources) {
			args = args.map(a => {
				if (a.Name !== "TOKEN" && a.Value.startsWith("ns/") && !a.Value.startsWith(`ns/${selectedNamespace}`)) {
					bumpArgVersion(a.Name);
					return { ...a, Value: "" };
				}
				return a;
			});
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

			// Reset argOptions and external versions when TTP changes
			argOptions = {};
			argExternalVersions = {};

			args = ttpParams?.map((param: TTPParam) => {
					let value = param.default;
					if (currentArgContext && param.name in currentArgContext) {
						value = currentArgContext[param.name];
					}

					if (value === '${TARGET}') {
						value = currentTargetId;
						if (param.type === 'string') {
							// if the type is string, then only the name of ther target is relevant
							const e = parseEntityId(currentTargetId);
							value = e?.name || '';
						}
						console.log("Setting target", param.name, "to value", value);
					}
					if (param.type === 'Namespace') {
						namespaceArgName = param.name;
						if (param.default === "" && currentTargetId.startsWith("ns/")) {
							value = currentTargetId.split("/")[1];
						}
					} else if (param.type === 'Pod') {
						const isSetTargetTTP = ttpTactic === 'Initial Access'; // special handling for the setTarget TTP, to use all available pods
						availableEntities = campaignState.getPods("", isSetTargetTTP)
						argOptions[param.name] = availableEntities.map(entityToComboboxOption);
					} else if (param.type === 'ServiceAccount') {
						// Use initialNamespace derived from targetId, not selectedNamespace
						availableEntities = campaignState.getServiceAccounts();
						argOptions[param.name] = availableEntities.map(entityToComboboxOption);
						console.info("Initialized ServiceAccount", argOptions[param.name]);
					}

					return {
						Name: param.name,
						Value: value,
						IsTrue: param.default === 'true',
						Description: param.description,
						Type: param.type,
						Required: param.required
					};
				}) || [];

			// Also reset procedureId when TTP changes
			procedureId = ttpProcedures?.[0]?.id || '';

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
					// for non-primitive types ensure the value is in the expected format
					console.group("Processing arg", arg.Name);
					console.info("Processing arg", arg.Name, "of type", arg.Type, "with value", arg.Value);
					
					// Entity types should use the full ID (not just the name)
					const entityTypes = [
						'Pod', 'Namespace', 'ServiceAccount', 'Service', 'Deployment', 'Container',
						'ConfigMap', 'Secret', 'Role', 'ClusterRole', 'RoleBinding', 'ClusterRoleBinding',
						'Node', 'ClusterNode', 'Ingress', 'Daemonset', 'CronJob', 'Job', 'Statefulset',
						'Volume', 'User', 'Group', 'KubeApiServer', 'ControlPlane',
						// GCP resources
						'GCPBucket', 'GCPServiceAccount', 'GCPServiceAccountToken', 'MetadataServer', 'GCPMetadataServer'
					];
					const isEntityType = entityTypes.includes(arg.Type);
					
					if (!isEntityType && arg.Name.toLowerCase().endsWith("name") && arg.Value.indexOf("/") !== -1) {
						// For non-entity types, if the arg is a name, extract just the name part
						const parts = arg.Value.split("/");
						arg.Value = parts[parts.length - 1];
					}
					// Otherwise keep the full value (ID for entities, or whatever was provided)
					
					console.info("Post Processed arg", arg.Name, "final value", arg.Value);
					console.groupEnd();
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
		if (selectedNamespace !== "" && argName !== namespaceArgName && argName !== "TOKEN") {
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

	function onArgChange(arg: Arg, e) {
		// If the chosen item carries a namespace in its group, set it
		if (arg.Type !== 'Namespace') {
			const ns = e.items?.[0]?.group;
			if (ns) {selectNamespace(ns);}
		}
		console.info("Arg change event for", arg.Name, "new value", e.value[0], "selected item", e.items?.[0]);
		// IMMUTABLE UPDATE so Svelte sees it:
		const i = args.findIndex(a => a.Name === arg.Name);
		if (i !== -1) {
			const newValue = e.value[0];
			args = args.with(i, { ...args[i], Value: newValue });
		} else {
			console.warn("Could not find arg to update:", arg.Name);
		}

		console.info("Updated args after change:", args);

		// arg.Value = e.value[0]
		// if (e.items.length > 0 && e.items[0].group ) {
		// 	selectNamespace(e.items[0].group);
		// }
	}

	function handleInputBlur(arg: Arg, inputValue: string) {
		const i = args.findIndex(a => a.Name === arg.Name);
		if (i === -1) return;

		const currentArg = args[i];
		
		// Get available options for this arg
		let options = argOptions[arg.Name] ?? [];
		if (selectedNamespace !== "" && arg.Name !== namespaceArgName && arg.Name !== "TOKEN") {
			options = options.filter(o => o.group === selectedNamespace);
		}
		
		// Check if the input matches an option's label or value
		const matchingOption = options.find((opt: ComboboxOption) => 
			opt.label === inputValue || opt.value === inputValue
		);
		
		if (matchingOption) {
			// Use the full value (ID) from the matching option
			args = args.with(i, { ...currentArg, Value: matchingOption.value });
		} else if (inputValue !== currentArg.Value) {
			// Custom value typed, not from dropdown
			args = args.with(i, { ...currentArg, Value: inputValue });
		}
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

<form class="w-full space-y-8" onsubmit={onInternalExecute}>
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
					{#each ttp.procedures as procedure (procedure.id)}
						<option
							value={procedure.id}
							disabled={executingSystemHasTool(targetId, procedure.id)}
							>{procedure.id}
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
				class="checkbox ml-8"
				bind:checked={arg.IsTrue}
				type="checkbox"
				placeholder={arg.Description}
			/>
		{:else if getArgOptions(arg.Name).length > 0}
			<Combobox
				collection={toComboBoxCollection(getArgOptions(arg.Name))}
				onValueChange={(e) => onArgChange(arg, e)}
				inputBehavior="autocomplete"
				allowCustomValue={true}
				openOnChange={true}
				defaultValue={[arg.Value]}
				placeholder={arg.Name + "..."}
				>
				<Combobox.Control>
					<Combobox.Input onblur={(e) => handleInputBlur(arg, e.currentTarget.value)} />
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
