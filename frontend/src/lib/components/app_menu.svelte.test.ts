import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { uiPreferences } from '#lib/stores/uiPreferences.svelte.js';
import AppMenu from './app_menu.svelte';

describe('AppMenu settings', () => {
	beforeEach(() => {
		uiPreferences.setAutoOpenReadFiles(true);
	});

	it('toggles whether read files open automatically', async () => {
		render(AppMenu, {
			context: new Map([
				[
					'$_campaignState',
					{
						reset: vi.fn(),
						GetFlow: vi.fn()
					}
				]
			])
		});

		await fireEvent.click(screen.getByRole('button', { name: 'Ran' }));
		const setting = await screen.findByRole('menuitemcheckbox', {
			name: 'Show read files'
		});

		expect(setting).toHaveAttribute('aria-checked', 'true');
		expect(setting.lastElementChild).toHaveTextContent('✓');
		await fireEvent.click(setting);
		expect(uiPreferences.autoOpenReadFiles).toBe(false);
	});
});
