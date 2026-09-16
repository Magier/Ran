import type { NamespaceUiConfig } from '$lib/api/index';

export const DEFAULT_NAMESPACE_UI_CONFIG: NamespaceUiConfig = {
	excluded: ['kube-system', 'local-path-storage'],
	included: []
};

export type NamespaceFilterOverrides = {
	showAll: boolean;
	namespaces: Record<string, boolean>;
};

export const DEFAULT_NAMESPACE_FILTER_OVERRIDES: NamespaceFilterOverrides = {
	showAll: false,
	namespaces: {}
};

export function configuredNamespaceHidden(namespace: string, config: NamespaceUiConfig): boolean {
	if (config.included.length > 0) return !config.included.includes(namespace);
	return config.excluded.includes(namespace);
}

export function namespaceHidden(
	namespace: string,
	config: NamespaceUiConfig,
	overrides: NamespaceFilterOverrides
): boolean {
	if (Object.hasOwn(overrides.namespaces, namespace)) return overrides.namespaces[namespace];
	return !overrides.showAll && configuredNamespaceHidden(namespace, config);
}

export function hiddenNamespaces(
	availableNamespaces: Iterable<string>,
	config: NamespaceUiConfig,
	overrides: NamespaceFilterOverrides
): Set<string> {
	return new Set(
		[...availableNamespaces].filter((namespace) => namespaceHidden(namespace, config, overrides))
	);
}

export function toggleNamespaceOverride(
	namespace: string,
	config: NamespaceUiConfig,
	overrides: NamespaceFilterOverrides
): NamespaceFilterOverrides {
	const currentlyHidden = namespaceHidden(namespace, config, overrides);
	const nextNamespaces = { ...overrides.namespaces };
	const configuredHidden = !overrides.showAll && configuredNamespaceHidden(namespace, config);

	if (!currentlyHidden === configuredHidden) {
		delete nextNamespaces[namespace];
	} else {
		nextNamespaces[namespace] = !currentlyHidden;
	}

	return { ...overrides, namespaces: nextNamespaces };
}

export function clearAllNamespaceFilters(): NamespaceFilterOverrides {
	return { showAll: true, namespaces: {} };
}

export function restoreConfiguredNamespaceFilters(): NamespaceFilterOverrides {
	return DEFAULT_NAMESPACE_FILTER_OVERRIDES;
}

export function parseNamespaceFilterOverrides(raw: string | null): NamespaceFilterOverrides {
	if (!raw) return DEFAULT_NAMESPACE_FILTER_OVERRIDES;
	try {
		const parsed: unknown = JSON.parse(raw);
		if (typeof parsed !== 'object' || parsed === null) return DEFAULT_NAMESPACE_FILTER_OVERRIDES;
		const candidate = parsed as Partial<NamespaceFilterOverrides>;
		if (
			typeof candidate.showAll !== 'boolean' ||
			typeof candidate.namespaces !== 'object' ||
			candidate.namespaces === null ||
			Array.isArray(candidate.namespaces) ||
			Object.values(candidate.namespaces).some((hidden) => typeof hidden !== 'boolean')
		) {
			return DEFAULT_NAMESPACE_FILTER_OVERRIDES;
		}
		return { showAll: candidate.showAll, namespaces: candidate.namespaces };
	} catch {
		return DEFAULT_NAMESPACE_FILTER_OVERRIDES;
	}
}
