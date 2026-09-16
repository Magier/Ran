import { describe, expect, it } from 'vitest';
import { executionFailureMessage } from './CampaignState.svelte';

describe('executionFailureMessage', () => {
	it('shows the runtime error without classifier wording', () => {
		expect(
			executionFailureMessage(
				"unclassified failure: nsenter: reassociate to namespace 'ns/ipc' failed: Operation not permitted"
			)
		).toBe("nsenter: reassociate to namespace 'ns/ipc' failed: Operation not permitted");
	});

	it('keeps known failure details unchanged', () => {
		expect(executionFailureMessage('access denied by RBAC or runtime policy')).toBe(
			'access denied by RBAC or runtime policy'
		);
	});
});
