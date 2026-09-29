import { describe, expect, it, vi } from 'vitest';
import { UiPreferences } from './uiPreferences.svelte';

describe('UiPreferences', () => {
	it('opens read files automatically by default', () => {
		const storage = { getItem: vi.fn(() => null), setItem: vi.fn() };

		expect(new UiPreferences(storage).autoOpenReadFiles).toBe(true);
	});

	it('restores a disabled auto-open preference', () => {
		const storage = { getItem: vi.fn(() => 'false'), setItem: vi.fn() };

		expect(new UiPreferences(storage).autoOpenReadFiles).toBe(false);
	});

	it('updates and persists the auto-open preference', () => {
		const storage = { getItem: vi.fn(() => null), setItem: vi.fn() };
		const preferences = new UiPreferences(storage);

		preferences.setAutoOpenReadFiles(false);

		expect(preferences.autoOpenReadFiles).toBe(false);
		expect(storage.setItem).toHaveBeenCalledWith('ran.autoOpenReadFiles', 'false');
	});
});
