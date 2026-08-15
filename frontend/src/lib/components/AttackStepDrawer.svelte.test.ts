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
	startedAt: '2026-08-15T09:10:11Z',
	completedAt: '2026-08-15T09:10:12Z',
	executedOn: 'target-1',
	status: 'Success',
	success: true
};

function renderDrawer(onclose = vi.fn()) {
	return {
		...render(AttackStepDrawer, {
			props: { step, onclose },
			context: new Map([['$_campaignState', { getEntityById: () => ({ name: 'target-pod' }) }]])
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

	it('closes when its selected step is cleared', async () => {
		const { rerender } = renderDrawer();

		await rerender({ step: null });

		await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
	});
});
