import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import OperationTimeline from './OperationTimeline.svelte';
import type { ActionGroup, TopEntry } from '$lib/stores/timelineStore.svelte';

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

function renderTimeline(
	entries: TopEntry[],
	handlers: {
		onfocusentity?: (targetId: string) => void;
		ontogglegroup?: (cmdId: string) => void;
		onviewaction?: (cmdId: string) => void;
	} = {}
) {
	const onfocusentity = handlers.onfocusentity ?? vi.fn();
	const ontogglegroup = handlers.ontogglegroup ?? vi.fn();
	const onviewaction = handlers.onviewaction ?? vi.fn();
	const result = render(OperationTimeline, {
		entries,
		onfocusentity,
		ontogglegroup,
		onviewaction
	});
	return { ...result, onfocusentity, ontogglegroup, onviewaction };
}

function actionEntry(overrides: Partial<ActionGroup> = {}): ActionGroup {
	return {
		kind: 'action-group',
		action: {
			kind: 'ttp-action',
			id: 'action',
			ttpId: 'list-pods',
			ttpName: 'List pods',
			targetId: 'target',
			targetName: 'target-pod',
			status: 'success'
		},
		effects: [],
		collapsed: true,
		...overrides
	};
}

describe('OperationTimeline action interactions', () => {
	it('opens details from the TTP name without toggling the group', async () => {
		const { onviewaction, ontogglegroup } = renderTimeline([actionEntry()]);

		await fireEvent.click(screen.getByRole('button', { name: 'List pods' }));

		expect(onviewaction).toHaveBeenCalledWith('action');
		expect(ontogglegroup).not.toHaveBeenCalled();
	});

	it('still toggles the group when the action row itself is clicked', async () => {
		const { onviewaction, ontogglegroup } = renderTimeline([actionEntry()]);
		const row = screen.getByRole('button', { name: 'List pods' }).closest('[aria-expanded]');

		expect(row).not.toBeNull();
		await fireEvent.click(row!);

		expect(ontogglegroup).toHaveBeenCalledWith('action');
		expect(onviewaction).not.toHaveBeenCalled();
	});

	it('focuses a target without opening details or toggling the group', async () => {
		const { onfocusentity, onviewaction, ontogglegroup } = renderTimeline([actionEntry()]);

		await fireEvent.click(screen.getByRole('button', { name: 'target-pod' }));

		expect(onfocusentity).toHaveBeenCalledWith('target');
		expect(onviewaction).not.toHaveBeenCalled();
		expect(ontogglegroup).not.toHaveBeenCalled();
	});

	it('keeps startup action names as plain text', () => {
		renderTimeline([
			actionEntry({
				action: {
					kind: 'ttp-action',
					id: 'startup',
					ttpId: '',
					ttpName: 'Read kubeconfig',
					targetId: '',
					targetName: '',
					status: 'success',
					startup: true,
					detail: 'developer'
				}
			})
		]);

		expect(screen.getByText('Read kubeconfig')).not.toBeInstanceOf(HTMLButtonElement);
		expect(screen.queryByRole('button', { name: 'Read kubeconfig' })).not.toBeInTheDocument();
	});
});

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
