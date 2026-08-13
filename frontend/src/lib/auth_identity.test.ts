import { describe, expect, it } from 'vitest';

import { selectDefaultAuthIdentity } from './auth_identity';

const identities = [{ id: 'ns/default/sa/reader' }, { id: 'k8s/credential/operator' }];

describe('selectDefaultAuthIdentity', () => {
	it('prefers the target identity', () => {
		expect(selectDefaultAuthIdentity(identities, identities[1].id, identities[0].id)).toBe(
			identities[1].id
		);
	});

	it('selects a sole eligible identity', () => {
		expect(selectDefaultAuthIdentity([identities[0]], 'ns/default/pod/demo')).toBe(
			identities[0].id
		);
	});

	it('uses the ranked ServiceAccount only when it is eligible', () => {
		expect(selectDefaultAuthIdentity(identities, 'ns/default/pod/demo', identities[0].id)).toBe(
			identities[0].id
		);
		expect(selectDefaultAuthIdentity(identities, 'ns/default/pod/demo', 'missing')).toBe('');
	});
});
