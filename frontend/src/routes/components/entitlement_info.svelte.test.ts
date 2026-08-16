import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';

import type { KubetierCatalog, RBACPermission } from '$lib/api';
import EntitlementInfo from './entitlement_info.svelte';

const catalog: KubetierCatalog = {
	schemaVersion: 1,
	attribution: 'Assessment data by KubeTier (https://kubetier.com/)',
	sourceUrl: 'https://kubetier.com/llms.txt',
	fetchedAt: '2026-08-15T00:00:00Z',
	sourceSha256: 'abc',
	validatedKubernetesVersion: '1.36.1',
	full: false,
	permissions: [
		{
			id: 'secrets-list',
			verb: 'list',
			resource: 'secrets',
			apiGroup: '',
			scope: 'cluster',
			tier: 'T0',
			escalationCount: 1,
			sourceUrl: 'https://kubetier.com/secrets-list',
			kubernetesDocUrl: 'https://kubernetes.io/docs/reference/access-authn-authz/rbac/',
			escalationPaths: []
		},
		{
			id: 'secrets-list-namespaced',
			verb: 'list',
			resource: 'secrets',
			apiGroup: '',
			scope: 'namespaced',
			tier: 'T1',
			escalationCount: 3,
			sourceUrl: 'https://kubetier.com/secrets-list-namespaced',
			kubernetesDocUrl: 'https://kubernetes.io/docs/concepts/configuration/secret/',
			escalationPaths: []
		},
		{
			id: 'secrets-list-duplicate-source',
			verb: 'list',
			resource: 'secrets',
			apiGroup: '',
			scope: 'cluster',
			tier: 'T0',
			escalationCount: 1,
			sourceUrl: 'https://kubetier.com/secrets-list',
			kubernetesDocUrl: 'https://kubernetes.io/docs/reference/access-authn-authz/rbac/',
			escalationPaths: []
		},
		{
			id: 'wildcard-all',
			verb: '*',
			resource: '*',
			apiGroup: '*',
			scope: 'cluster',
			tier: 'T0',
			escalationCount: 0,
			sourceUrl: 'https://kubetier.com/wildcard-all',
			kubernetesDocUrl: 'https://kubernetes.io/docs/reference/access-authn-authz/rbac/#user-facing-roles',
			escalationPaths: []
		},
		{
			id: 'configmaps-get',
			verb: 'get',
			resource: 'configmaps',
			apiGroup: '',
			scope: 'namespaced',
			tier: 'T2',
			escalationCount: 0,
			sourceUrl: 'https://kubetier.com/configmaps-get',
			escalationPaths: []
		},
		{
			id: 'pods-get',
			verb: 'get',
			resource: 'pods',
			apiGroup: '',
			scope: 'namespaced',
			tier: 'T3',
			escalationCount: 0,
			sourceUrl: 'https://kubetier.com/pods-get',
			escalationPaths: []
		}
	],
	roles: [
		{
			id: 'cluster-admin',
			name: 'cluster-admin',
			scope: 'cluster',
			tier: 'T0',
			sourceUrl: 'https://kubetier.com/cluster-admin',
			rules: [
				{ apiGroups: ['*'], resources: ['*'], nonResourceUrls: [], verbs: ['*'] },
				{ apiGroups: [], resources: [], nonResourceUrls: ['*'], verbs: ['*'] }
			],
			notes: []
		}
	]
};

function permission(overrides: Partial<RBACPermission> = {}): RBACPermission {
	return {
		verb: 'list',
		resource: '',
		resourceType: 'secrets',
		resourceName: '',
		apiGroup: '',
		scope: '',
		sourceRole: '',
		isNamespaced: true,
		scopeKind: 'unknown',
		evaluatedNamespace: 'default',
		scopeSource: 'ssrr',
		...overrides
	};
}

describe('KubeTier entitlement presentation', () => {
	it('uses the strongest tier as capability color and moves evidence into the tooltip', async () => {
		render(EntitlementInfo, { props: { entitlements: [permission()], catalog } });
		expect(screen.queryByText(/^T[0-3](?:–T[0-3])?$/)).not.toBeInTheDocument();
		expect(screen.getByText('list secrets')).toHaveClass('text-red-700');
		expect(screen.queryByText('unverified')).not.toBeInTheDocument();
		expect(screen.queryByText('default')).not.toBeInTheDocument();
		await fireEvent.mouseEnter(screen.getByLabelText('Details for list secrets'));
		expect(screen.getByText('unverified')).toBeInTheDocument();
		expect(screen.getByText('default')).toBeInTheDocument();
		expect(screen.getByText('ssrr')).toBeInTheDocument();
		expect(screen.queryByText(/Assessment data by/)).not.toBeInTheDocument();
	});

	it('colors proven namespace permissions and leaves unknown permissions neutral', () => {
		render(EntitlementInfo, {
			props: {
				entitlements: [
					permission({ scopeKind: 'namespace', scope: 'default', scopeSource: 'binding' }),
					permission({ verb: 'dance', resourceType: 'pods' })
				],
				catalog
			}
		});
		expect(screen.getByText('list secrets')).toHaveClass('text-orange-700');
		expect(screen.getByText('dance pods')).toHaveClass('text-surface-500');
		expect(screen.queryByText('Unassessed')).not.toBeInTheDocument();
	});

	it('uses green for T2 and gray for T3', () => {
		render(EntitlementInfo, {
			props: {
				entitlements: [
					permission({ verb: 'get', resourceType: 'configmaps', scopeKind: 'namespace', scope: 'default' }),
					permission({ verb: 'get', resourceType: 'pods', scopeKind: 'namespace', scope: 'default' })
				],
				catalog
			}
		});

		expect(screen.getByText('get configmaps')).toHaveClass('text-green-700');
		expect(screen.getByText('get pods')).toHaveClass('text-surface-500');
	});

	it('deduplicates assessment sources inside the capability tooltip', async () => {
		render(EntitlementInfo, { props: { entitlements: [permission()], catalog } });
		await fireEvent.mouseEnter(screen.getByLabelText('Details for list secrets'));

		expect(screen.getAllByRole('link', { name: 'KubeTier: list secrets (cluster)' })).toHaveLength(1);
		expect(screen.getAllByRole('link', { name: 'KubeTier: list secrets (namespaced)' })).toHaveLength(1);
		expect(screen.getAllByRole('link', { name: 'Kubernetes docs: list secrets (cluster)' })).toHaveLength(1);
		expect(screen.getAllByRole('link', { name: 'Kubernetes docs: list secrets (namespaced)' })).toHaveLength(1);
		expect(screen.queryByText(/Assessment by KubeTier/)).not.toBeInTheDocument();
		expect(screen.getByRole('link', { name: 'KubeTier: list secrets (cluster)' })).not.toHaveClass('font-semibold');
	});

	it('maps an SSRR wildcard capability exclusively to the dedicated KubeTier entry', async () => {
		render(EntitlementInfo, {
			props: {
				entitlements: [permission({ verb: '*', resourceType: '*', apiGroup: '*', scopeKind: 'unknown' })],
				catalog
			}
		});
		await fireEvent.mouseEnter(screen.getByLabelText('Details for * *'));

		expect(screen.getAllByRole('link', { name: 'KubeTier assessment' })).toHaveLength(1);
		expect(screen.getByRole('link', { name: 'KubeTier assessment' })).toHaveAttribute('href', 'https://kubetier.com/reference/?p=wildcard-all');
		expect(screen.getAllByRole('link', { name: 'Kubernetes documentation' })).toHaveLength(1);
	});

	it('folds the non-resource wildcard into a universal resource wildcard', () => {
		render(EntitlementInfo, {
			props: {
				entitlements: [
					permission({ verb: '*', resourceType: '*', apiGroup: '*', scopeKind: 'unknown' }),
					permission({
						verb: '*',
						resourceType: '',
						resourceName: '*',
						apiGroup: '',
						scope: '*',
						scopeKind: 'cluster'
					}),
					permission({ verb: 'get', resourceType: '', resourceName: '/healthz', scope: '*', scopeKind: 'cluster' }),
					permission({ verb: 'get', resourceType: '', resourceName: '/healthz', scope: '*', scopeKind: 'cluster' })
				],
				catalog
			}
		});

		expect(screen.getAllByText('* *')).toHaveLength(1);
		expect(screen.getAllByLabelText('Details for * *')).toHaveLength(1);
		expect(screen.getAllByText('get /healthz')).toHaveLength(1);
	});

	it('recognizes an exact built-in ClusterRole fingerprint', () => {
		render(EntitlementInfo, {
			props: {
				entitlements: [
					permission({ verb: '*', resourceType: '*', apiGroup: '*', scopeKind: 'unknown' }),
					permission({ verb: '*', resourceType: '', resourceName: '*', apiGroup: '' })
				],
				catalog,
				roleName: 'cluster-admin',
				roleKind: 'ClusterRole'
			}
		});
		expect(screen.getByText('KubeTier built-in Role assessment ↗')).toBeInTheDocument();
		expect(screen.getByText('Discovered rules match the imported reference.')).toBeInTheDocument();
	});

	it('warns on built-in drift but does not tier a custom Role with the same name', () => {
		const entitlements = [permission({ verb: '*', resourceType: '*', apiGroup: '*' })];
		const { unmount } = render(EntitlementInfo, {
			props: { entitlements, catalog, roleName: 'cluster-admin', roleKind: 'ClusterRole' }
		});
		expect(screen.getByText(/Definition differs from the imported Kubernetes/)).toBeInTheDocument();
		unmount();

		render(EntitlementInfo, {
			props: { entitlements, catalog, roleName: 'cluster-admin', roleKind: 'Role' }
		});
		expect(screen.queryByText('KubeTier built-in Role assessment ↗')).not.toBeInTheDocument();
	});
});
