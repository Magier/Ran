import { render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { AttackStep } from '$lib/api';
import AttackStepDrawer from './AttackStepDrawer.svelte';

const step: AttackStep = {
	id: 'cmd-1',
	targetId: 'target-1',
	command: 'id',
	traversal: [],
	innerCommand: '',
	args: {},
	procedureId: 'shell',
	TTP: {
		id: 'whoami',
		name: 'Who am I',
		description: 'Identify the current user',
		tactic: 'Discovery',
		techniques: ['T1033']
	},
	results: ['uid=1000'],
	stdout: 'uid=1000',
	stderr: '',
	outputTruncated: false,
	outputSequence: 0,
	stdoutBytes: 8,
	stderrBytes: 0,
	startedAt: '2026-08-15T09:10:11Z',
	completedAt: '2026-08-15T09:10:12Z',
	executedOn: 'target-1',
	status: 'Success',
	success: true
};

function renderDrawer(
	onclose = vi.fn(),
	selectedStep: AttackStep = step,
	output: Record<string, unknown> | undefined = undefined
) {
	return {
		...render(AttackStepDrawer, {
			props: { step: selectedStep, onclose },
			context: new Map([
				[
					'$_campaignState',
					{ getEntityById: () => ({ name: 'target-pod' }), getExecutionOutput: () => output }
				]
			])
		}),
		onclose
	};
}

describe('AttackStepDrawer', () => {
	it('renders the shared attack-step details', () => {
		renderDrawer();

		expect(screen.getByRole('dialog')).toBeInTheDocument();
		expect(screen.getByRole('heading', { name: 'Who am I' })).toBeInTheDocument();
		expect(screen.getByText('uid=1000')).toBeInTheDocument();
	});

	it('shows output received while an action is still running', () => {
		const ongoing: AttackStep = { ...step, status: 'Ongoing', success: false, completedAt: '' };
		renderDrawer(vi.fn(), ongoing, {
			sequence: 2,
			stdout: 'Nmap scan report for 10.0.0.5\nHost is up',
			stderr: '',
			stdoutBytes: 45,
			stderrBytes: 0,
			truncated: false,
			completed: false
		});

		expect(screen.getByText('Live · 45 bytes')).toBeInTheDocument();
		expect(screen.getByText(/Nmap scan report for 10\.0\.0\.5/)).toBeInTheDocument();
	});

	it('shows completion for an action that produced no output', () => {
		const ongoing: AttackStep = {
			...step,
			results: [],
			stdout: '',
			status: 'Ongoing',
			success: false,
			completedAt: ''
		};
		renderDrawer(vi.fn(), ongoing, {
			sequence: 0,
			stdout: '',
			stderr: '',
			stdoutBytes: 0,
			stderrBytes: 0,
			truncated: false,
			completed: true,
			success: true
		});

		expect(screen.getByText('Success')).toBeInTheDocument();
		expect(screen.getByText('Completed without output.')).toBeInTheDocument();
		expect(screen.queryByText(/Waiting for output/)).not.toBeInTheDocument();
	});

	it('closes when its selected step is cleared', async () => {
		const { rerender } = renderDrawer();

		await rerender({ step: null });

		await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
	});
});
