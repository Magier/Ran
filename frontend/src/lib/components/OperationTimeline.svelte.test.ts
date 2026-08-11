import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import OperationTimeline from './OperationTimeline.svelte';
import type { TopEntry } from '$lib/stores/timelineStore.svelte';

function compactTimestamp(date: Date): string {
	return date.toLocaleTimeString([], {
		hour: '2-digit',
		minute: '2-digit',
		hour12: false
	});
}

function fullTimestamp(date: Date): string {
	return date.toLocaleString([], { dateStyle: 'medium', timeStyle: 'long' });
}

function renderTimeline(entries: TopEntry[]) {
	return render(OperationTimeline, {
		entries,
		onfocusentity: vi.fn(),
		ontogglegroup: vi.fn()
	});
}

describe('OperationTimeline timestamps', () => {
	it('shows compact timestamps with full hover text for every row type', async () => {
		const actionTimestamp = new Date(2026, 4, 25, 12, 34, 56);
		const effectTimestamp = new Date(2026, 4, 25, 12, 35, 57);
		const standaloneTimestamp = new Date(2026, 4, 25, 12, 36, 58);
		const entries: TopEntry[] = [
			{
				kind: 'discovery',
				id: 'standalone',
				entityId: 'standalone',
				entityName: 'standalone-pod',
				entityKind: 'Pod',
				timestamp: standaloneTimestamp
			},
			{
				kind: 'action-group',
				action: {
					kind: 'ttp-action',
					id: 'action',
					ttpId: 'list-pods',
					ttpName: 'List pods',
					targetId: 'target',
					targetName: 'target-pod',
					status: 'success',
					timestamp: actionTimestamp
				},
				effects: [
					{
						kind: 'discovery',
						id: 'effect',
						entityId: 'effect',
						entityName: 'effect-pod',
						entityKind: 'Pod',
						timestamp: effectTimestamp
					}
				],
				collapsed: false
			}
		];

		renderTimeline(entries);

		for (const timestamp of [actionTimestamp, effectTimestamp, standaloneTimestamp]) {
			const full = fullTimestamp(timestamp);
			const trigger = screen.getByText(compactTimestamp(timestamp));
			expect(trigger).toHaveAttribute('aria-label', full);

			await fireEvent.mouseEnter(trigger);
			expect(screen.getByRole('tooltip')).toHaveTextContent(full);

			await fireEvent.mouseLeave(trigger);
			expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
		}
	});

	it('keeps the Startup label and shows its full timestamp on hover', async () => {
		const startupTimestamp = new Date(2026, 4, 25, 12, 37, 59);
		renderTimeline([
			{
				kind: 'action-group',
				action: {
					kind: 'ttp-action',
					id: 'startup',
					ttpId: '',
					ttpName: 'Read kubeconfig',
					targetId: '',
					targetName: '',
					status: 'success',
					startup: true,
					detail: 'developer',
					timestamp: startupTimestamp
				},
				effects: [],
				collapsed: true
			}
		]);

		const trigger = screen.getByText('Startup');
		await fireEvent.mouseEnter(trigger);
		expect(screen.getByRole('tooltip')).toHaveTextContent(fullTimestamp(startupTimestamp));
	});
});
