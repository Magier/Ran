import { render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import type { TTP } from '$lib/api';
import ActionParamsModal from './ActionParamsModal.svelte';

const ttp = {
	id: 'target-derived-action',
	name: 'Target-derived action',
	tactic: 'Lateral Movement',
	params: [
		{
			name: 'TARGET',
			type: 'string',
			default: '${TARGET.IP}',
			description: 'Target address'
		}
	],
	procedures: [{ id: 'execute', command: 'tool --target ${TARGET}' }]
} as TTP;

function targetInput(): HTMLInputElement {
	const label = screen.getByText('TARGET');
	const input = label.parentElement?.querySelector('input');
	expect(input).toBeInstanceOf(HTMLInputElement);
	return input as HTMLInputElement;
}

describe('ActionParamsModal target-derived defaults', () => {
	it('re-grounds the same TTP when its target changes', async () => {
		const entities = new Map([
			['pod/first', { id: 'pod/first', name: 'first', ips: ['10.0.0.6'] }],
			['pod/second', { id: 'pod/second', name: 'second', ips: ['10.0.0.11'] }]
		]);
		const campaignState = {
			relations: new Map(),
			graph: { nodes: [] },
			getObjectById: (id: string) => entities.get(id),
			getCompromisedSystems: () => [],
			getPods: () => [],
			getServiceAccounts: () => [],
			getServiceAccountsWithTokens: () => []
		};

		const props = {
			targetId: 'pod/first',
			ttp,
			argContext: {},
			onExecute: vi.fn(),
			onCancel: vi.fn()
		};
		const view = render(ActionParamsModal, {
			props,
			context: new Map([['$_campaignState', campaignState]])
		});

		await waitFor(() => expect(targetInput()).toHaveValue('10.0.0.6'));
		await view.rerender({ ...props, targetId: 'pod/second' });
		await waitFor(() => expect(targetInput()).toHaveValue('10.0.0.11'));
	});

	it('defaults source-side procedures away from their semantic target', async () => {
		const redis = {
			id: 'pod/oopservability/redis',
			name: 'redis',
			namespace: 'oopservability',
			ips: ['10.0.0.11']
		};
		const foothold = {
			id: 'pod/default/foothold',
			name: 'foothold',
			namespace: 'default',
			ips: ['10.0.0.6']
		};
		const campaignState = {
			relations: new Map(),
			graph: { nodes: [] },
			getObjectById: (id: string) => (id === redis.id ? redis : foothold),
			getCompromisedSystems: () => [redis, foothold],
			getPods: () => [],
			getServiceAccounts: () => [],
			getServiceAccountsWithTokens: () => []
		};
		const sourceSideTtp = {
			...ttp,
			procedures: [
				{
					id: 'redis-cli',
					tool: 'redis-cli',
					runOnTarget: false,
					command: 'redis-cli -h ${TARGET}'
				}
			]
		} as TTP;

		render(ActionParamsModal, {
			props: {
				targetId: redis.id,
				ttp: sourceSideTtp,
				argContext: {},
				onExecute: vi.fn(),
				onCancel: vi.fn()
			},
			context: new Map([['$_campaignState', campaignState]])
		});

		expect(await screen.findByText('Execute From')).toBeInTheDocument();
		const selector = screen.getByRole('combobox', { name: 'Execute From' });
		expect(selector).toHaveValue('');
		expect(screen.getByRole('option', { name: 'Automatic reachable system' })).toBeInTheDocument();
		expect(screen.getByRole('option', { name: 'default/foothold' })).toBeInTheDocument();
		expect(screen.queryByRole('option', { name: 'oopservability/redis' })).not.toBeInTheDocument();
	});
});
