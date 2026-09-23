import { render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import FilesystemTree from './filesystem_tree.svelte';

function renderTree(
	volumeMounts: {
		mountPoint: string;
		name: string;
		readOnly: boolean;
		isHostPath: boolean;
		origin: 'configured' | 'runtime';
	}[]
) {
	return render(FilesystemTree, {
		props: { objectId: 'system/unknown', canList: false, volumeMounts },
		context: new Map([
			[
				'$_campaignState',
				{
					api: {
						on: vi.fn(),
						off: vi.fn()
					}
				}
			]
		])
	});
}

describe('FilesystemTree mount observations', () => {
	it('keeps a root runtime mount visible outside the intentionally unbadged root', () => {
		renderTree([
			{
				mountPoint: '/',
				name: '',
				readOnly: false,
				isHostPath: false,
				origin: 'runtime'
			}
		]);

		expect(screen.getByText('Mounts (1)')).toBeInTheDocument();
		expect(screen.getByLabelText('Known mounts')).toHaveTextContent('/');
		expect(screen.getByText('runtime mount')).toBeInTheDocument();
	});
});
