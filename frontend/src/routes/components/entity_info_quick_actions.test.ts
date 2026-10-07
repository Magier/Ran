import { describe, expect, it } from 'vitest';
import type { TTP } from '$lib/api/index';
import { quickActionFields, quickActionsForField } from './entity_info_quick_actions';

function tokenAction(id: string, runOnTarget?: boolean): TTP {
	return {
		id,
		name: id,
		description: '',
		tactic: 'Credential Access',
		techniques: [],
		status: 'enabled',
		params: [],
		requires: {},
		effects: ['rawServiceaccountToken'],
		procedures: [{ id: 'read-token', command: 'cat token', runOnTarget }]
	};
}

describe('quickActionsForField', () => {
	it('uses applicable target-local actions for a field shortcut', () => {
		const direct = tokenAction('read-service-account-token');
		const sourceSide = tokenAction('extract-serviceaccount-token-via-cve', false);

		expect(quickActionsForField('service_account_name', 'Pod', [sourceSide, direct])).toEqual([
			direct
		]);
	});

	it('does not offer a source-side action as an entity field shortcut', () => {
		expect(
			quickActionsForField('service_account_name', 'Pod', [
				tokenAction('extract-serviceaccount-token-via-cve', false)
			])
		).toEqual([]);
	});

	it('returns every direct applicable action instead of selecting by Armory order', () => {
		const first = tokenAction('first');
		const second = tokenAction('second');

		expect(quickActionsForField('service_account_name', 'Pod', [second, first])).toEqual([
			second,
			first
		]);
	});

	it('keeps the service account field exclusion', () => {
		expect(
			quickActionsForField('service_account_name', 'ServiceAccount', [tokenAction('read')])
		).toEqual([]);
	});

	it('keeps runtime mount discovery available to the filesystem control', () => {
		const action = { ...tokenAction('get-volume-mounts'), effects: ['executor::linux.mounts'] };
		expect(quickActionsForField('mounts', 'UnknownSystem', [action])).toEqual([action]);
	});

	it('maps every migrated physical effect declaration to its entity fields', () => {
		const declarations: Array<[string, string[]]> = [
			['executor::linux.mounts', ['mounts']],
			['executor::sys.envVar', ['envVars']],
			['executor::sys.ip', ['ips']],
			['executor::sys.files', ['files', 'binaries']],
			['executor::sys.userID', ['user_id']]
		];
		const actions = declarations.map(([effect], index) => ({
			...tokenAction(`physical-${index}`),
			effects: [effect]
		}));

		for (const [effect, fields] of declarations) {
			const action = actions.find((candidate) => candidate.effects?.includes(effect));
			for (const field of fields) {
				expect(quickActionsForField(field, 'Pod', actions)).toContain(action);
			}
		}
		expect(quickActionFields('Pod', actions)).toEqual(
			new Set(['mounts', 'envVars', 'ips', 'files', 'binaries', 'user_id'])
		);
	});

	it('uses the declared subject when the executor differs from the selected entity', () => {
		const sourceSide = { ...tokenAction('upload', false), effects: ['target::sys.files'] };
		const executorSide = { ...tokenAction('inspect', false), effects: ['executor::sys.files'] };

		expect(quickActionsForField('files', 'Pod', [sourceSide, executorSide])).toEqual([sourceSide]);
	});

	it('excludes local executor observations but retains explicit target observations', () => {
		for (const effect of ['executor::sys.files', 'sys.files']) {
			const local = tokenAction('local-files');
			local.procedures[0].isLocalCommand = true;
			local.effects = [effect];
			const targetBound = { ...local, id: 'target-files', effects: ['target::sys.files'] };
			expect(quickActionsForField('files', 'Pod', [local, targetBound])).toEqual([targetBound]);
			expect(quickActionFields('Pod', [local])).toEqual(new Set());
		}
	});

	it('requires every procedure to stay on target for executor-bound shortcuts', () => {
		const mixed = tokenAction('mixed-files');
		mixed.effects = ['executor::sys.files'];
		mixed.procedures.push({ id: 'local', command: 'ls', isLocalCommand: true });
		expect(quickActionsForField('files', 'Pod', [mixed])).toEqual([]);
	});

	it('never offers a declarative action without a procedure, regardless of effect subject', () => {
		for (const effect of ['target::sys.files', 'executor::sys.files', 'sys.files']) {
			const declarative = { ...tokenAction('no-procedures'), procedures: [], effects: [effect] };
			expect(quickActionsForField('files', 'Pod', [declarative])).toEqual([]);
			expect(quickActionFields('Pod', [declarative])).toEqual(new Set());
		}
	});

	it('excludes automatically planned clients from executor-bound shortcuts', () => {
		const structuredHttp = tokenAction('http-client');
		structuredHttp.effects = ['executor::sys.ip'];
		structuredHttp.procedures[0].http_request = { url: 'https://example.test' };

		const structuredK8s = tokenAction('k8s-client');
		structuredK8s.effects = ['executor::sys.ip'];
		structuredK8s.procedures[0].k8s_request = { path: '/api/v1/pods' };

		const authenticatedShell = tokenAction('authenticated-shell');
		authenticatedShell.effects = ['executor::sys.ip'];
		authenticatedShell.procedures[0].command = 'curl -H "Authorization: Bearer ${K8S_AUTH}"';

		expect(
			quickActionsForField('ips', 'Node', [structuredHttp, structuredK8s, authenticatedShell])
		).toEqual([]);
	});

	it('retains explicitly target-pinned client observations', () => {
		const targetClient = tokenAction('target-client', true);
		targetClient.effects = ['executor::sys.ip'];
		targetClient.procedures[0].http_request = { url: 'https://example.test' };

		expect(quickActionsForField('ips', 'Node', [targetClient])).toEqual([targetClient]);
	});

	it('keeps identity-bound permission review shortcuts for client procedures', () => {
		const permissionReview = tokenAction('check-sa-token-permissions');
		permissionReview.effects = ['k8s.SelfSubjectRulesReview'];
		permissionReview.procedures = [
			{ id: 'kubectl', command: 'kubectl auth can-i --list ${K8S_AUTH}' },
			{
				id: 'http',
				command: '',
				http_request: {
					authentication: '${K8S_AUTH}',
					method: 'POST',
					url: 'https://kubernetes.default/apis/authorization.k8s.io/v1/selfsubjectrulesreviews'
				}
			}
		];

		expect(quickActionsForField('can', 'ServiceAccount', [permissionReview])).toEqual([
			permissionReview
		]);
		expect(quickActionFields('ServiceAccount', [permissionReview])).toEqual(new Set(['can']));
	});
});
