import { describe, expect, it } from 'vitest';
import { actionDisplayName } from './actionDisplay';

describe('actionDisplayName', () => {
	it('renders any declared argument placeholder', () => {
		expect(
			actionDisplayName('Connect to ${HOST || target}', 'Connect to target', {
				HOST: 'db.internal'
			})
		).toBe('Connect to db.internal');
	});

	it('uses the declared fallback when arguments are not known in the Armory', () => {
		expect(actionDisplayName('Install ${PKG || package}', 'Install Package')).toBe(
			'Install package'
		);
	});

	it('never puts a sensitive argument value in a title', () => {
		expect(
			actionDisplayName('Authenticate with ${TOKEN || credential}', 'Authenticate', {
				TOKEN: 'sensitive-value'
			})
		).toBe('Authenticate with credential');
	});
});
