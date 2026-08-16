<script lang="ts">
	import Icon from '@iconify/svelte';
	import type {
		KubetierCatalog,
		KubetierPermission,
		KubetierRole,
		KubetierTier,
		RBACPermission
	} from '$lib/api';

	type Props = {
		entitlements: RBACPermission[];
		catalog?: KubetierCatalog | null;
		roleName?: string;
		roleKind?: string;
	};

	let { entitlements, catalog = null, roleName, roleKind }: Props = $props();
	type TooltipState = {
		permission: RBACPermission;
		left: number;
		top?: number;
		bottom?: number;
	};
	type DocumentationLink = {
		url: string;
		assessment: KubetierPermission;
	};

	let activeTooltip = $state<TooltipState | null>(null);
	let hideTooltipTimer: ReturnType<typeof setTimeout> | undefined;

	const tierRank: Record<KubetierTier, number> = { T0: 0, T1: 1, T2: 2, T3: 3 };

	function tierClass(tier: KubetierTier): string {
		return {
			T0: 'border-red-400 bg-red-100 text-red-800 dark:bg-red-950 dark:text-red-200',
			T1: 'border-orange-400 bg-orange-100 text-orange-800 dark:bg-orange-950 dark:text-orange-200',
			T2: 'border-green-400 bg-green-100 text-green-800 dark:bg-green-950 dark:text-green-200',
			T3: 'border-surface-400 bg-surface-100 text-surface-700 dark:bg-surface-800 dark:text-surface-300'
		}[tier];
	}

	function permissionTierClass(tier: KubetierTier | undefined): string {
		if (!tier) return 'text-surface-500';
		return {
			T0: 'font-bold text-red-700 dark:text-red-300',
			T1: 'font-semibold text-orange-700 dark:text-orange-300',
			T2: 'text-green-700 dark:text-green-300',
			T3: 'text-surface-500 dark:text-surface-400'
		}[tier];
	}

	function normalizedGroup(permission: RBACPermission): string {
		return permission.apiGroup ?? '';
	}

	function permissionResource(permission: RBACPermission): string {
		return permission.resourceType ?? permission.resource ?? '';
	}

	function nonResourceMatches(catalogResource: string, url: string): boolean {
		if (!catalogResource.startsWith('nonResourceURLs:')) return false;
		if (url === '*') return true;
		return catalogResource
			.slice('nonResourceURLs:'.length)
			.split(',')
			.some((pattern) => {
				const clean = pattern.trim();
				return clean.endsWith('*') ? url.startsWith(clean.slice(0, -1)) : clean === url;
			});
	}

	function matchingAssessments(permission: RBACPermission): KubetierPermission[] {
		if (!catalog) return [];
		const resource = permissionResource(permission);
		const nonResourceUrl = !resource && (permission.resourceName?.startsWith('/') || permission.resourceName === '*')
			? permission.resourceName
			: null;
		const scopeKind = permission.scopeKind ?? 'unknown';
		return catalog.permissions.filter((assessment) => {
			const universalWildcard = permission.verb === '*' && resource === '*';
			const verbMatches = universalWildcard
				? assessment.verb === '*'
				: permission.verb === '*' || assessment.verb === permission.verb;
			const resourceMatches = nonResourceUrl
				? nonResourceMatches(assessment.resource, nonResourceUrl)
				: universalWildcard
					? assessment.resource === '*'
					: resource === '*' || assessment.resource === resource;
			const groupMatches = universalWildcard
				? assessment.apiGroup === '*'
				: nonResourceUrl !== null || normalizedGroup(permission) === '*'
					? true
					: assessment.apiGroup === normalizedGroup(permission);
			const scopeMatches = universalWildcard || (scopeKind === 'unknown'
				? true
				: scopeKind === 'cluster'
					? assessment.scope === 'cluster'
					: assessment.scope === 'namespaced');
			return verbMatches && resourceMatches && groupMatches && scopeMatches;
		});
	}

	function tiersFor(permission: RBACPermission): KubetierTier[] {
		return [...new Set(matchingAssessments(permission).map((entry) => entry.tier))]
			.sort((a, b) => tierRank[a] - tierRank[b]);
	}

	function scopeLabel(permission: RBACPermission): string {
		if (permission.scopeKind === 'cluster') return 'cluster-wide';
		if (permission.scopeKind === 'namespace') return `namespace ${permission.scope ?? ''}`.trim();
		return 'unverified';
	}

	function displayResource(permission: RBACPermission): string {
		const resource = permissionResource(permission);
		if (!resource && permission.resourceName) return permission.resourceName;
		return `${resource}${permission.resourceName ? `/${permission.resourceName}` : ''}`;
	}

	function isUniversalResourcePermission(permission: RBACPermission): boolean {
		return permission.verb === '*'
			&& permissionResource(permission) === '*'
			&& normalizedGroup(permission) === '*';
	}

	function isUniversalNonResourcePermission(permission: RBACPermission): boolean {
		return permission.verb === '*'
			&& permissionResource(permission) === ''
			&& permission.resourceName === '*';
	}

	function permissionIdentity(permission: RBACPermission): string {
		return JSON.stringify([
			permission.verb,
			permissionResource(permission),
			permission.resourceName ?? '',
			normalizedGroup(permission),
			permission.scope ?? '',
			permission.scopeKind ?? 'unknown',
			permission.evaluatedNamespace ?? '',
			permission.scopeSource ?? '',
			permission.sourceRole ?? ''
		]);
	}

	function sortedEntitlements(): RBACPermission[] {
		const hasUniversalResourcePermission = entitlements.some(isUniversalResourcePermission);
		const filteredEntitlements = hasUniversalResourcePermission
			? entitlements.filter((permission) => !isUniversalNonResourcePermission(permission))
			: entitlements;
		const visibleEntitlements = [...new Map(
			filteredEntitlements.map((permission) => [permissionIdentity(permission), permission])
		).values()];
		return [...visibleEntitlements].sort((a, b) => {
			const aTier = tiersFor(a)[0];
			const bTier = tiersFor(b)[0];
			const rank = (aTier ? tierRank[aTier] : 99) - (bTier ? tierRank[bTier] : 99);
			return rank || `${a.verb} ${displayResource(a)}`.localeCompare(`${b.verb} ${displayResource(b)}`);
		});
	}

	function uniqueAssessments(matches: KubetierPermission[]): KubetierPermission[] {
		return [...new Map(matches.map((assessment) => [assessment.sourceUrl, assessment])).values()];
	}

	function uniqueDocumentationLinks(matches: KubetierPermission[]): DocumentationLink[] {
		const links = new Map<string, DocumentationLink>();
		for (const assessment of matches) {
			if (assessment.kubernetesDocUrl && !links.has(assessment.kubernetesDocUrl)) {
				links.set(assessment.kubernetesDocUrl, {
					url: assessment.kubernetesDocUrl,
					assessment
				});
			}
		}
		return [...links.values()];
	}

	function assessmentLabel(assessment: KubetierPermission, assessments: KubetierPermission[]): string {
		const base = `${assessment.verb} ${assessment.resource}`;
		const hasScopeVariants = assessments.filter((entry) =>
			entry.verb === assessment.verb && entry.resource === assessment.resource
		).length > 1;
		return hasScopeVariants ? `${base} (${assessment.scope})` : base;
	}

	function assessmentUrl(assessment: KubetierPermission): string {
		return assessment.id === 'wildcard-all'
			? 'https://kubetier.com/reference/?p=wildcard-all'
			: assessment.sourceUrl;
	}

	function uniqueDescriptions(matches: KubetierPermission[]): string[] {
		return [...new Set(matches.flatMap((assessment) => assessment.description ?? []))];
	}

	function uniqueEscalationPaths(matches: KubetierPermission[]): KubetierPermission['escalationPaths'] {
		return [...new Map(
			matches.flatMap((assessment) => assessment.escalationPaths).map((path) => [path.sourceUrl, path])
		).values()];
	}

	function escalationCount(matches: KubetierPermission[]): number {
		return Math.max(0, ...matches.map((assessment) => assessment.escalationCount));
	}

	function clearTooltipTimer(): void {
		if (hideTooltipTimer) clearTimeout(hideTooltipTimer);
		hideTooltipTimer = undefined;
	}

	function showTooltip(event: MouseEvent | FocusEvent, permission: RBACPermission): void {
		clearTooltipTimer();
		const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
		const width = Math.min(448, window.innerWidth - 16);
		const left = Math.max(8, Math.min(rect.left, window.innerWidth - width - 8));
		if (rect.top > window.innerHeight / 2) {
			activeTooltip = { permission, left, bottom: window.innerHeight - rect.top + 6 };
		} else {
			activeTooltip = { permission, left, top: rect.bottom + 6 };
		}
	}

	function scheduleTooltipHide(): void {
		clearTooltipTimer();
		hideTooltipTimer = setTimeout(() => {
			activeTooltip = null;
		}, 120);
	}

	function roleAssessment(): KubetierRole | undefined {
		// Kubernetes' built-ins are ClusterRoles. A custom namespaced Role may
		// legitimately reuse a built-in name and must not inherit its overall tier.
		return roleKind === 'ClusterRole' && roleName
			? catalog?.roles.find((role) => role.name === roleName)
			: undefined;
	}

	function actualRuleSet(): Set<string> {
		return new Set(entitlements.map((permission) => {
			const resource = permissionResource(permission);
			const target = resource || `url:${permission.resourceName ?? ''}`;
			const resourceName = resource ? permission.resourceName ?? '' : '';
			return `${normalizedGroup(permission)}|${target}|${permission.verb}|${resourceName}`;
		}));
	}

	function referenceRuleSet(role: KubetierRole): Set<string> {
		const entries: string[] = [];
		for (const rule of role.rules) {
			for (const verb of rule.verbs) {
				for (const group of rule.apiGroups.length ? rule.apiGroups : ['']) {
					for (const resource of rule.resources) entries.push(`${group}|${resource}|${verb}|`);
				}
				for (const url of rule.nonResourceUrls) entries.push(`|url:${url}|${verb}|`);
			}
		}
		return new Set(entries);
	}

	function roleMatches(role: KubetierRole): boolean {
		const actual = actualRuleSet();
		const reference = referenceRuleSet(role);
		return actual.size === reference.size && [...actual].every((entry) => reference.has(entry));
	}

	const builtInRole = $derived.by(() => roleAssessment());
</script>

{#if builtInRole}
	<div class="mb-3 rounded border border-surface-300 p-2 dark:border-surface-700">
		<div class="flex flex-wrap items-center gap-2">
			<span class={`rounded border px-1.5 py-0.5 font-mono font-bold ${tierClass(builtInRole.tier)}`}>
				{builtInRole.tier}
			</span>
			<a class="font-semibold underline" href={builtInRole.sourceUrl} target="_blank" rel="noreferrer">
				KubeTier built-in Role assessment ↗
			</a>
		</div>
		{#if roleMatches(builtInRole)}
			<p class="mt-1 text-green-700 dark:text-green-300">Discovered rules match the imported reference.</p>
		{:else}
			<p class="mt-1 font-semibold text-amber-700 dark:text-amber-300">
				Definition differs from the imported Kubernetes {catalog?.validatedKubernetesVersion ?? ''} reference; this is a nominal tier only.
			</p>
		{/if}
		{#if builtInRole.description}<p class="mt-1 text-surface-600 dark:text-surface-300">{builtInRole.description}</p>{/if}
	</div>
{/if}

<div class="max-h-80 overflow-auto pl-2">
	{#each sortedEntitlements() as permission}
		{@const tiers = tiersFor(permission)}
		<div class="flex items-center gap-1">
			<span class={`font-mono ${permissionTierClass(tiers[0])}`}>
				{permission.verb} {displayResource(permission)}
			</span>
			<button
				type="button"
				class="inline-flex cursor-help items-center text-surface-400 hover:text-surface-700 focus:outline-none dark:hover:text-surface-200"
				aria-label={`Details for ${permission.verb} ${displayResource(permission)}`}
				onmouseenter={(event) => showTooltip(event, permission)}
				onmouseleave={scheduleTooltipHide}
				onfocus={(event) => showTooltip(event, permission)}
				onblur={scheduleTooltipHide}
			>
				<Icon icon="mdi:help-circle-outline" width="13" />
			</button>
		</div>
	{/each}
</div>

{#if activeTooltip}
	{@const matches = matchingAssessments(activeTooltip.permission)}
	{@const assessments = uniqueAssessments(matches)}
	{@const documentationLinks = uniqueDocumentationLinks(matches)}
	{@const descriptions = uniqueDescriptions(matches)}
	{@const escalationPaths = uniqueEscalationPaths(matches)}
	{@const documentedEscalations = escalationCount(matches)}
	<div
		role="dialog"
		aria-label="Capability details"
		tabindex="-1"
		class="fixed z-50 max-h-[70vh] w-[min(28rem,calc(100vw-1rem))] overflow-auto rounded-md border border-surface-300 bg-surface-50 p-3 text-sm font-normal text-surface-700 shadow-xl dark:border-surface-600 dark:bg-surface-900 dark:text-surface-200"
		style:left={`${activeTooltip.left}px`}
		style:top={activeTooltip.top === undefined ? undefined : `${activeTooltip.top}px`}
		style:bottom={activeTooltip.bottom === undefined ? undefined : `${activeTooltip.bottom}px`}
		onmouseenter={clearTooltipTimer}
		onmouseleave={scheduleTooltipHide}
		onfocusin={clearTooltipTimer}
		onfocusout={scheduleTooltipHide}
	>
		<div class="font-mono font-semibold">
			{activeTooltip.permission.verb} {displayResource(activeTooltip.permission)}
		</div>
		<dl class="mt-2 grid grid-cols-[auto_1fr] gap-x-2 gap-y-1 text-xs">
			<dt class="text-surface-500">Scope</dt>
			<dd>{scopeLabel(activeTooltip.permission)}</dd>
			{#if activeTooltip.permission.evaluatedNamespace}
				<dt class="text-surface-500">Evaluated in</dt>
				<dd>{activeTooltip.permission.evaluatedNamespace}</dd>
			{/if}
			{#if activeTooltip.permission.scopeSource}
				<dt class="text-surface-500">Evidence</dt>
				<dd>{activeTooltip.permission.scopeSource}</dd>
			{/if}
			<dt class="text-surface-500">API group</dt>
			<dd><code>{activeTooltip.permission.apiGroup || '(core)'}</code></dd>
		</dl>
		{#if matches.length === 0}
			<p class="mt-2 text-xs text-surface-500">No matching KubeTier assessment.</p>
		{:else}
			<div class="mt-3 flex flex-wrap gap-x-3 gap-y-1 text-xs">
				{#each assessments as assessment}
					<a
						class="underline"
						href={assessmentUrl(assessment)}
						target="_blank"
						rel="noreferrer"
						aria-label={assessments.length === 1 ? 'KubeTier assessment' : `KubeTier: ${assessmentLabel(assessment, assessments)}`}
					>{assessments.length === 1 ? 'KubeTier assessment' : `KubeTier: ${assessmentLabel(assessment, assessments)}`} ↗</a>
				{/each}
				{#each documentationLinks as documentationLink}
					<a
						class="underline"
						href={documentationLink.url}
						target="_blank"
						rel="noreferrer"
						aria-label={documentationLinks.length === 1 ? 'Kubernetes documentation' : `Kubernetes docs: ${assessmentLabel(documentationLink.assessment, assessments)}`}
					>
						{documentationLinks.length === 1 ? 'Kubernetes documentation' : `Kubernetes docs: ${assessmentLabel(documentationLink.assessment, assessments)}`} ↗
					</a>
				{/each}
			</div>
			{#each descriptions as description}<p class="mt-2">{description}</p>{/each}
			{#if escalationPaths.length > 0}
				<ul class="mt-2 list-disc pl-4">
					{#each escalationPaths as path}
						<li>
							<a class="underline" href={path.sourceUrl} target="_blank" rel="noreferrer">{path.name} ↗</a>
							{#if path.steps.length > 0}<ol class="list-decimal pl-4">{#each path.steps as step}<li>{step}</li>{/each}</ol>{/if}
						</li>
					{/each}
				</ul>
			{:else if documentedEscalations > 0}
				<p class="mt-2 text-xs text-surface-500">{#if matches.length > 1}Up to {/if}{documentedEscalations} documented escalation path(s).</p>
			{/if}
		{/if}
	</div>
{/if}
