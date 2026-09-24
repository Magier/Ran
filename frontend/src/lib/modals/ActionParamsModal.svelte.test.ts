import { render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import type { ActionResolution, TTP } from '$lib/api';
import { getRanAPI } from '$lib/ran_api';
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

	it('uses resolution candidates instead of silently choosing the first target IP', async () => {
		const target = {
			id: 'pod/first',
			name: 'first',
			ips: ['10.0.0.6', '192.0.2.7']
		};
		const campaignState = {
			relations: new Map(),
			graph: { nodes: [] },
			getObjectById: () => target,
			getCompromisedSystems: () => [],
			getPods: () => [],
			getServiceAccounts: () => [],
			getServiceAccountsWithTokens: () => []
		};
		const resolution: ActionResolution = {
			actionId: ttp.id,
			targetId: target.id,
			status: 'needs_choice',
			reasons: ['TARGET has multiple values derived from TARGET.IP'],
			procedures: [{ procedureId: 'execute', status: 'unknown' }],
			recommendedProcedureId: 'execute',
			arguments: [
				{
					name: 'TARGET',
					type: 'string',
					required: true,
					status: 'needs_choice',
					reason: 'TARGET has multiple values derived from TARGET.IP',
					candidates: target.ips.map((ip) => ({
						value: ip,
						label: ip,
						source: {
							kind: 'target_fact',
							entityId: target.id,
							field: 'system.ips',
							expression: '${TARGET.IP}'
						}
					}))
				}
			]
		};
		const targetAwareTtp = {
			...ttp,
			actionState: {
				status: 'needs_choice',
				reasons: resolution.reasons,
				arguments: { total: 1, resolved: 0, needsInput: 0, needsChoice: 1, blocked: 0 },
				procedures: resolution.procedures,
				recommendedProcedureId: resolution.recommendedProcedureId
			}
		} as TTP;
		const resolutionSpy = vi
			.spyOn(getRanAPI(), 'GetActionResolution')
			.mockResolvedValue(resolution);

		render(ActionParamsModal, {
			props: {
				targetId: target.id,
				ttp: targetAwareTtp,
				argContext: {},
				onExecute: vi.fn(),
				onCancel: vi.fn()
			},
			context: new Map([['$_campaignState', campaignState]])
		});

		await waitFor(() => expect(targetInput()).toHaveValue(''));
		expect(resolutionSpy).toHaveBeenCalledWith(ttp.id, target.id, undefined);
		const resolutionInfo = screen.getByLabelText(/Resolution for TARGET/);
		expect(resolutionInfo).toHaveAttribute('title', expect.stringContaining('system.ips'));
		expect(resolutionInfo).not.toHaveClass('ig-cell');
		expect(resolutionInfo.closest('.input-group')).toBeNull();
		resolutionSpy.mockRestore();
	});

	it('uses backend procedure readiness to select an available fallback', async () => {
		const target = {
			id: 'pod/default/demo',
			name: 'demo',
			namespace: 'default'
		};
		const procedures = [
			{ id: 'ip', tool: 'ip', command: 'ip address' },
			{ id: 'hostname', tool: 'hostname', command: 'hostname -i' }
		];
		const procedureStates = [
			{
				procedureId: 'ip',
				status: 'unavailable' as const,
				requiredTool: 'ip',
				reason: "required tool 'ip' is known to be absent from the execution system"
			},
			{
				procedureId: 'hostname',
				status: 'ready' as const,
				requiredTool: 'hostname'
			}
		];
		const targetAwareTtp = {
			...ttp,
			procedures,
			actionState: {
				status: 'ready',
				reasons: [],
				arguments: { total: 0, resolved: 0, needsInput: 0, needsChoice: 0, blocked: 0 },
				procedures: procedureStates,
				recommendedProcedureId: 'hostname'
			}
		} as TTP;
		const resolution: ActionResolution = {
			actionId: targetAwareTtp.id,
			targetId: target.id,
			status: 'ready',
			reasons: [],
			arguments: [],
			procedures: procedureStates,
			recommendedProcedureId: 'hostname'
		};
		const campaignState = {
			relations: new Map(),
			graph: { nodes: [] },
			getObjectById: () => target,
			getCompromisedSystems: () => [target],
			getPods: () => [],
			getServiceAccounts: () => [],
			getServiceAccountsWithTokens: () => []
		};
		const resolutionSpy = vi
			.spyOn(getRanAPI(), 'GetActionResolution')
			.mockResolvedValue(resolution);

		render(ActionParamsModal, {
			props: {
				targetId: target.id,
				ttp: targetAwareTtp,
				argContext: {},
				onExecute: vi.fn(),
				onCancel: vi.fn()
			},
			context: new Map([['$_campaignState', campaignState]])
		});

		const selector = screen.getByLabelText('Procedure') as HTMLSelectElement;
		await waitFor(() => expect(selector).toHaveValue('hostname'));
		expect(screen.getByRole('option', { name: 'ip ❌' })).toBeDisabled();
		expect(resolutionSpy).toHaveBeenLastCalledWith(targetAwareTtp.id, target.id, target.id);
		resolutionSpy.mockRestore();
	});
});
