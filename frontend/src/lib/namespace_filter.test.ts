import { describe, expect, it } from 'vitest';
import {
	clearAllNamespaceFilters,
	hiddenNamespaces,
	parseNamespaceFilterOverrides,
	restoreConfiguredNamespaceFilters,
	toggleNamespaceOverride
} from './namespace_filter';

describe('namespace graph filtering', () => {
	it('hides every configured blacklist namespace', () => {
		const hidden = hiddenNamespaces(
			['default', 'kube-system', 'kube-public', 'oopservability'],
			{ excluded: ['kube-system', 'kube-public', 'oopservability'], included: [] },
			restoreConfiguredNamespaceFilters()
		);

		expect(hidden).toEqual(new Set(['kube-system', 'kube-public', 'oopservability']));
	});

	it('applies an allowlist to namespaces discovered after startup', () => {
		const config = { excluded: ['ignored'], included: ['production'] };
		const overrides = restoreConfiguredNamespaceFilters();

		expect(hiddenNamespaces(['production'], config, overrides)).toEqual(new Set());
		expect(hiddenNamespaces(['production', 'staging'], config, overrides)).toEqual(
			new Set(['staging'])
		);
	});

	it('persists only explicit user choices over configured defaults', () => {
		const config = { excluded: ['kube-system'], included: [] };
		const visibleOverride = toggleNamespaceOverride(
			'kube-system',
			config,
			restoreConfiguredNamespaceFilters()
		);

		expect(hiddenNamespaces(['kube-system'], config, visibleOverride)).toEqual(new Set());
		expect(toggleNamespaceOverride('kube-system', config, visibleOverride)).toEqual(
			restoreConfiguredNamespaceFilters()
		);
	});

	it('clears all configured and explicit filters for the session', () => {
		const config = { excluded: ['kube-system'], included: [] };
		expect(
			hiddenNamespaces(['kube-system', 'default'], config, clearAllNamespaceFilters())
		).toEqual(new Set());
	});

	it('falls back safely for malformed stored overrides', () => {
		expect(parseNamespaceFilterOverrides('{not json')).toEqual(restoreConfiguredNamespaceFilters());
		expect(
			parseNamespaceFilterOverrides('{"showAll": true, "namespaces": {"default": "yes"}}')
		).toEqual(restoreConfiguredNamespaceFilters());
	});
});
