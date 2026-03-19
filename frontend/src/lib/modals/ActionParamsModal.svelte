<script lang="ts">
	import { Combobox, useListCollection } from '@skeletonlabs/skeleton-svelte';

	import { parseEntityId } from '$lib/model';
	import type { TTP, TTPParam } from '$lib/api/index';
	import { getCampaignState, type Entity } from '$lib/components/CampaignState.svelte';
	import { untrack } from 'svelte';

	interface ParamProps {
		targetId: string;
		ttp: TTP;
		argContext: Record<string, any>;
		onExecute: (ttpId: string, execSystemId: string, procedureId: string, args: Record<string, string>) => void;
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
	let selectedExecSystemId = $state('');
	let formElement: HTMLFormElement | undefined = $state();

	const compromisedSystems = $derived(campaignState.getCompromisedSystems());
	const execSystemOptions = $derived<ComboboxOption[]>(
		compromisedSystems.map(e => ({ label: e.name, value: e.id, group: e.namespace }))
	);
	const selectedExecSystem = $derived(
		compromisedSystems.find(e => e.id === selectedExecSystemId)
	);

	const target = $derived.by(() => { return campaignState.getObjectById(targetId); });

	let argOptions: Record<string, ComboboxOption[]> = $state({});
	// Incremented when a combobox value is set programmatically (external update),
	// used as a {#key} to force remount so defaultValue is re-applied.
	let argExternalVersions: Record<string, number> = $state({});
	
	// Track which args have been auto-selected to prevent infinite loops
	let autoSelectedArgs: Set<string> = new Set();

	function bumpArgVersion(name: string) {
		argExternalVersions = { ...argExternalVersions, [name]: (argExternalVersions[name] ?? 0) + 1 };
	}

	// Track previous TTP ID to detect when TTP changes
	let previousTtpId: string | undefined = undefined;

	// Track the last TTP ID we focused for, to only focus once per TTP
	let lastFocusedTTPId = $state<string | undefined>(undefined);

	let selectedNamespace = $derived.by(() => {
		const nsArg = args.find(arg => arg.Type === 'Namespace');
		return nsArg ? nsArg.Value : '';
	});
	
	// Track the last namespace we cleared for to prevent repeated clears
	let lastClearedNamespace = $state('');

	let isAllNamespaces = $derived.by(() => {
		const allNsArg = args.find(arg => arg.Name === 'ALL_NS');
		return allNsArg ? allNsArg.IsTrue : false;
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
		// Only run when namespace actually changes
		if (selectedNamespace === lastClearedNamespace) {
			return;
		}
		
		// Update tracking before processing
		lastClearedNamespace = selectedNamespace;
		
		// if the namespace changes, clear out-of-namespace resources (except TOKEN which can be cross-namespace)
		let hasOutOfNsResources = false;
		
		for (const arg of args) {
			// Skip namespace arg itself and TOKEN (which can be cross-namespace)
			if (arg.Type === 'Namespace' || arg.Name === 'TOKEN') continue;
			
			// Only check args that have a value
			if (!arg.Value) continue;
			
			// Check if this arg has options (meaning it's an entity selector)
			const options = argOptions[arg.Name];
			if (options && options.length > 0) {
				// Find the selected option to get its namespace (group)
				const selectedOption = options.find(opt => opt.value === arg.Value);
				if (selectedOption && selectedOption.group && selectedOption.group !== selectedNamespace) {
					hasOutOfNsResources = true;
					break;
				}
			} else if (arg.Value.startsWith("ns/") && !arg.Value.startsWith(`ns/${selectedNamespace}`)) {
				// Fallback: check ID-based resources
				hasOutOfNsResources = true;
				break;
			}
		}

		if (hasOutOfNsResources) {
			args = args.map(a => {
				// Skip namespace arg itself and TOKEN
				if (a.Type === 'Namespace' || a.Name === 'TOKEN') return a;
				if (!a.Value) return a;
				
				// Check if this arg has options
				const options = argOptions[a.Name];
				if (options && options.length > 0) {
					const selectedOption = options.find(opt => opt.value === a.Value);
					if (selectedOption && selectedOption.group && selectedOption.group !== selectedNamespace) {
						bumpArgVersion(a.Name);
						// Reset auto-select tracking so it can re-select in the new namespace
						autoSelectedArgs.delete(a.Name);
						return { ...a, Value: "" };
					}
				} else if (a.Value.startsWith("ns/") && !a.Value.startsWith(`ns/${selectedNamespace}`)) {
					// Fallback: clear ID-based resources
					bumpArgVersion(a.Name);
					// Reset auto-select tracking so it can re-select in the new namespace
					autoSelectedArgs.delete(a.Name);
					return { ...a, Value: "" };
				}
				
				return a;
			});
		}
	});

	// Auto-select when only one option is available
	$effect(() => {
		// Only track args array changes, not derived values
		const currentArgs = args;
		
		// Use untrack to avoid circular dependencies with selectedNamespace
		untrack(() => {
			// Check each arg to see if it has exactly one option
			let needsUpdate = false;
			const updates: Array<{ index: number; value: string; name: string }> = [];
			
			currentArgs.forEach((arg, i) => {
				// Skip if it's a boolean or doesn't have options
				if (arg.Type === 'bool') return;
				
				// Skip if we've already auto-selected this arg
				if (autoSelectedArgs.has(arg.Name)) return;
				
				// Skip if already has a value
				if (arg.Value && arg.Value !== '') return;
				
				const options = getArgOptions(arg.Name);
				
				// If there's exactly one option and current value doesn't match it
				if (options.length === 1 && arg.Value !== options[0].value) {
					updates.push({ index: i, value: options[0].value, name: arg.Name });
					needsUpdate = true;
				}
			});
			
			if (needsUpdate) {
				// Mark these args as auto-selected before updating
				updates.forEach(u => autoSelectedArgs.add(u.name));
				
				args = currentArgs.map((a, i) => {
					const update = updates.find(u => u.index === i);
					if (update) {
						console.info("Auto-selecting single option for", update.name, ":", update.value);
						bumpArgVersion(update.name);
						return { ...a, Value: update.value };
					}
					return a;
				});
			}
		});
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

			// Reset argOptions, external versions, auto-select tracking, and namespace tracking when TTP changes
			argOptions = {};
			argExternalVersions = {};
			autoSelectedArgs = new Set();
			lastClearedNamespace = '';

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
					} else if (value.indexOf('${TARGET.IP}') >= 0 && target?.ips?.length > 0) {
						value = value.replace("${TARGET.IP}", target.ips[0].IP);
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
						// For TOKEN params, only show ServiceAccounts that have extracted tokens (compromised)
						if (param.name === 'TOKEN') {
							availableEntities = campaignState.getServiceAccountsWithTokens();
						} else {
							availableEntities = campaignState.getServiceAccounts();
						}
						argOptions[param.name] = availableEntities.map(entityToComboboxOption);
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

			// Default execution system: prefer target if compromised, else first available
			const systems = campaignState.getCompromisedSystems();
			if (systems.some(s => s.id === currentTargetId)) {
				selectedExecSystemId = currentTargetId;
			} else if (systems.length > 0) {
				selectedExecSystemId = systems[0].id;
			} else {
				selectedExecSystemId = '';
			}

			console.log(args);
			console.groupEnd();
		});
	});

	// Focus first input once when TTP changes (modal opens with new TTP)
	$effect(() => {
		// Only focus if this is a new TTP
		if (ttp.id !== lastFocusedTTPId && args.length > 0 && formElement) {
			lastFocusedTTPId = ttp.id;
			// Use a small timeout to ensure DOM has updated
			setTimeout(() => {
				// Find the first input, checkbox, or combobox input within the params section
				const firstInput = formElement?.querySelector<HTMLInputElement | HTMLSelectElement>(
					'.input-group input:not([readonly]), .input-group input[type="checkbox"]'
				);
				firstInput?.focus();
			}, 50);
		}
	});

	// namespace options
	$effect(() => {
		const uniqueNamespaces = availableEntities.reduce((nss: Set<string>, r) => {
			if (r.namespace) { nss.add(r.namespace); }
			return nss;
		}, new Set<string>());
		argOptions[namespaceArgName] = Array.from(uniqueNamespaces.values()).map(ns => ({ label: ns, value: ns }));
	});

	// When the execution system changes, auto-switch to first available procedure if current is disabled
	$effect(() => {
		// track selectedExecSystemId to re-evaluate
		const _sys = selectedExecSystemId;
		const procedures = ttp?.procedures;
		if (!procedures || procedures.length <= 1) return;

		const currentOk = executingSystemHasTool(procedureToolName(
			procedures.find(p => p.id === procedureId) ?? procedures[0]
		));
		if (!currentOk) {
			const first = procedures.find(p => executingSystemHasTool(procedureToolName(p)));
			if (first) {
				procedureId = first.id;
			}
		}
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
					
					// If parameter name ends with "Name", send the label (name) instead of the ID
					if (arg.Name.endsWith("Name")) {
						const options = argOptions[arg.Name];
						if (options) {
							const matchingOption = options.find(opt => opt.value === arg.Value);
							if (matchingOption) {
								arg.Value = matchingOption.label;
								console.info("Parameter ends with 'Name': using label instead of ID:", matchingOption.label);
							}
						}
					} else {
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
					}
					
					console.info("Post Processed arg", arg.Name, "final value", arg.Value);
					console.groupEnd();
				}
				acc[arg.Name] = arg.Value;
				return acc;
			},
			{} as { [key: string]: string }
		);

		onExecute(ttp.id, selectedExecSystemId, procedureId, argsDict);
	}

	function executingSystemHasTool(tool: string): boolean {
		if (!selectedExecSystem?.binaries) return true; // no info, assume available
		const path = selectedExecSystem.binaries[tool];
		if (path === undefined) return true; // binary not tracked, assume available
		return path !== '' && path !== '❌';
	}

	function procedureToolName(procedure: { id: string; tool?: string }): string {
		return procedure.tool || procedure.id;
	}

	function getArgOptions(argName: string): ComboboxOption[] {
		let opts = argOptions[argName] ?? [];
		if (selectedNamespace !== "" && argName !== namespaceArgName && argName !== "TOKEN") {
			opts = opts.filter(o => o.group === selectedNamespace);
		}
		return opts
	}
	
	function toComboBoxCollection(items: ComboboxOption[]) {
		return useListCollection({
		  items: items,
		  itemToString: (item) => item.label,
		  itemToValue: (item) => item.value,
		  groupBy: (item) => item.group || ''
		})
	}

	function onArgChange(arg: Arg, e: any) {
		console.info("Arg change event for", arg.Name, "new value", e.value[0], "selected item", e.items?.[0]);
		
		// IMMUTABLE UPDATE so Svelte sees it:
		const i = args.findIndex(a => a.Name === arg.Name);
		if (i !== -1) {
			const newValue = e.value[0];
			args = args.with(i, { ...args[i], Value: newValue });
		} else {
			console.warn("Could not find arg to update:", arg.Name);
		}
		
		// If the chosen item carries a namespace in its group, and this is not the Namespace field itself, auto-select that namespace
		if (arg.Type !== 'Namespace') {
			const selectedItem = e.items?.[0];
			const ns = selectedItem?.group;
			if (ns && ns !== selectedNamespace) {
				selectNamespace(ns);
			}
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
</script>

<form bind:this={formElement} class="w-full flex flex-col text-xs md:text-sm lg:text-base min-h-0" onsubmit={onInternalExecute}>
	<header class="flex justify-between flex-shrink-0">
		<h4 class="h4 text-sm md:text-base lg:text-lg">{ttp.name}</h4>
	</header>
	<article class="overflow-y-auto flex-1 min-h-0 space-y-4 pr-2">
		<div class="">
			<span class="h6 label text-xs md:text-sm lg:text-base">Description</span>
			{ttp.description}
		</div>
			{#if execSystemOptions.length > 0}
				<label class="label mt-5">
					<span class="h6 label-text text-xs md:text-sm lg:text-base">Execute On</span>
				{#if execSystemOptions.length === 1}
					<input id="execSystem" class="input mt-2 text-xs md:text-sm lg:text-base" value="{execSystemOptions[0].group}/{execSystemOptions[0].label}" readonly />
				{:else if execSystemOptions.length > 1}
					<select id="execSystem" class="input mt-2 text-xs md:text-sm lg:text-base" bind:value={selectedExecSystemId}>
						{#each execSystemOptions as sys (sys.value)}
							<option value={sys.value}>{sys.group}/{sys.label}</option>
						{/each}
					</select>
				{/if}
			</label>
			{/if}

			<label class="h6 label mt-5 text-xs md:text-sm lg:text-base" for="procedure">Procedure</label>
			{#if ttp.procedures && ttp.procedures.length > 1}
				<select id="procedure" class="input mt-2 text-xs md:text-sm lg:text-base" bind:value={procedureId} disabled={ttp.procedures.length <= 1}>
					{#each ttp.procedures as procedure (procedure.id)}
						<option
							value={procedure.id}
							disabled={!executingSystemHasTool(procedureToolName(procedure))}
							>{procedure.id}{!executingSystemHasTool(procedureToolName(procedure)) ? ' ❌' : ''}
						</option>
					{/each}
				</select>
			{:else}
				<code id="procedure" class="label mt-2 text-xs md:text-sm lg:text-base">{procedureId}</code>
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
					<span class="h5 text-xs md:text-sm lg:text-base">Params</span>
{#each args as arg, index (arg.Name)}
	<div class="input-group mt-2 grid-cols-[auto_1fr_auto] text-xs md:text-sm lg:text-base"
		class:opacity-50={arg.Type === 'Namespace' && isAllNamespaces}
		class:pointer-events-none={arg.Type === 'Namespace' && isAllNamespaces}
	>
		<div class="ig-cell preset-tonal">{arg.Name}</div>
		{#if arg.Type === 'bool'}
			<input
				class="checkbox ml-8"
				bind:checked={arg.IsTrue}
				type="checkbox"
				placeholder={arg.Description}
			/>
		{:else if getArgOptions(arg.Name).length > 0}
			{#key argExternalVersions[arg.Name] ?? 0}
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
						<Combobox.Content class="z-50 bg-surface-100-900 text-xs md:text-sm lg:text-base">
							{#each getArgOptions(arg.Name) as item (item)}
								<Combobox.Item {item} class="text-surface-contrast-100-900">
									<Combobox.ItemText >{item.label}</Combobox.ItemText>
									<Combobox.ItemIndicator />
								</Combobox.Item>
							{/each}
						</Combobox.Content>
					</Combobox.Positioner>
				</Combobox>
			{/key}
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
	<footer class="flex justify-end gap-4 flex-shrink-0 pt-4">
		<button type="button" class="btn preset-tonal text-xs md:text-sm lg:text-base" onclick={onCancel}>Cancel</button>
		<button type="submit" class="btn preset-filled-primary-300-700 text-xs md:text-sm lg:text-base">Execute</button>
	</footer>
</form>
