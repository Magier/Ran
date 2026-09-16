import { describe, expect, it } from 'vitest';
import type { TTP } from '$lib/api/index';
import { quickActionsForField } from './entity_info_quick_actions';

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
		const action = { ...tokenAction('get-volume-mounts'), effects: ['linux.mounts'] };
		expect(quickActionsForField('mounts', 'UnknownSystem', [action])).toEqual([action]);
	});
});
