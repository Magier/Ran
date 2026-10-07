import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { AttackStep } from '#lib/api/index.js';
import AttackStepDrawer from './AttackStepDrawer.svelte';

const step: AttackStep = {
	id: 'cmd-1',
	targetId: 'target-1',
	command: 'id',
	traversal: [],
	innerCommand: '',
	routeWarnings: [],
	reasoning: '',
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
					{
						getEntityById: () => ({ name: 'target-pod' }),
						getExecutionOutput: () => output,
						getTtpById: (id: string) =>
							id === 'install-package' ? { title: 'Install ${PKG || package}' } : undefined
					}
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

	it('shows operator reasoning in a collapsed disclosure', () => {
		renderDrawer(vi.fn(), {
			...step,
			reasoning: 'List pods before selecting a workload to inspect.'
		});

		const disclosure = screen.getByText('Reasoning').closest('details');
		expect(disclosure).not.toBeNull();
		expect(disclosure).not.toHaveAttribute('open');
		expect(
			screen.getByText('List pods before selecting a workload to inspect.')
		).toBeInTheDocument();
	});

	it('shows resolved parameters and uses the package name in install action titles', () => {
		renderDrawer(vi.fn(), {
			...step,
			args: { PKG: 'nmap', TOKEN: 'eyJheader.payload.signature' },
			reasoning: 'Install the scanner needed for the next step.',
			TTP: { ...step.TTP, id: 'install-package', name: 'Install Package' }
		});

		expect(screen.getByRole('heading', { name: 'Install nmap' })).toBeInTheDocument();
		const status = screen.getByText('Status');
		const parameters = screen.getByText('Parameters');
		const reasoning = screen.getByText('Reasoning');
		const disclosure = parameters.closest('details');
		const reasoningDisclosure = reasoning.closest('details');
		expect(disclosure).not.toBeNull();
		expect(disclosure).not.toHaveAttribute('open');
		expect(disclosure?.parentElement).toBe(reasoningDisclosure?.parentElement);
		expect(reasoningDisclosure).toHaveClass('mt-1');
		expect(
			status.compareDocumentPosition(parameters) & Node.DOCUMENT_POSITION_FOLLOWING
		).toBeTruthy();
		expect(
			parameters.compareDocumentPosition(reasoning) & Node.DOCUMENT_POSITION_FOLLOWING
		).toBeTruthy();
		expect(screen.getByText('nmap')).toBeInTheDocument();
		expect(screen.getByText('[redacted]')).toBeInTheDocument();
		expect(screen.queryByText('eyJheader.payload.signature')).not.toBeInTheDocument();
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

		expect(screen.getByLabelText('Follow output')).toBeChecked();
		expect(screen.getByText(/Nmap scan report for 10\.0\.0\.5/)).toBeInTheDocument();
	});

	it('shows exceptional route warnings inside traversal without a separate Route section', () => {
		renderDrawer(vi.fn(), {
			...step,
			routeWarnings: [
				{
					kind: 'broken-session-skipped',
					message: 'A broken session edge to the target was skipped.'
				}
			],
			traversal: [
				{
					fromId: 'c2/ran',
					toId: 'target-1',
					relation: 'exec',
					command: 'id'
				}
			],
			innerCommand: 'id'
		});

		expect(screen.getByText('Traversal')).toBeInTheDocument();
		expect(
			screen.getByText('A broken session edge to the target was skipped.')
		).toBeInTheDocument();
		expect(screen.queryByText('Route')).not.toBeInTheDocument();
	});

	it('shows one rendered hop command with its real nested data highlighted for legacy records', async () => {
		const command = 'runner --data "printf \\"hello\\""';
		renderDrawer(vi.fn(), {
			...step,
			traversal: [
				{
					fromId: 'system/source',
					toId: 'target-1',
					relation: 'rce.can-exec',
					envelope: 'runner --data "${CMD}"',
					command
				}
			],
			innerCommand: 'printf "hello"'
		});

		expect(screen.queryByText('Envelope')).not.toBeInTheDocument();
		expect(screen.queryByText('${CMD}')).not.toBeInTheDocument();
		const selectedHop = screen.getByRole('group', {
			name: 'Selected hop from source to target-1'
		});
		expect(selectedHop).toContainElement(screen.getByRole('button', { name: 'source' }));
		expect(selectedHop).toContainElement(screen.getByRole('button', { name: 'target-1' }));
		expect(screen.getByRole('button', { name: 'source' })).not.toHaveClass(
			'preset-filled-primary-500'
		);
		expect(screen.getByTitle('Nested command data')).toHaveTextContent('printf \\"hello\\"');
		expect(screen.getByTitle('Nested command data')).toHaveClass('text-primary-400');
		expect(screen.queryByText('nested data highlighted')).not.toBeInTheDocument();
		expect(screen.getByTitle('Nested command data').closest('code')).toHaveAttribute(
			'data-source',
			command
		);

		await fireEvent.click(screen.getByRole('button', { name: 'target-1' }));
		expect(screen.getByText('Command on target')).toBeInTheDocument();
		expect(screen.queryByText('runs on target')).not.toBeInTheDocument();
	});

	it('waits for output without showing the follow control', () => {
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
			completed: false
		});

		expect(screen.getByText('Waiting for output…')).toBeInTheDocument();
		expect(screen.queryByLabelText('Follow output')).not.toBeInTheDocument();
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
