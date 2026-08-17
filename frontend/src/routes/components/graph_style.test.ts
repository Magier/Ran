import { describe, expect, it } from 'vitest';

import { getGraphStyle, getK8sCredentialIcon } from './graph_style';

describe('getK8sCredentialIcon', () => {
	it('selects a contrasting icon for each graph theme', () => {
		expect(getK8sCredentialIcon(true)).toBe('/k8s/account-key-dark.svg');
		expect(getK8sCredentialIcon(false)).toBe('/k8s/account-key-light.svg');
	});

	it('uses the primary color for every selected node border', () => {
		const selectedNodeStyle = getGraphStyle(false).find(
			(rule: { selector: string }) => rule.selector === 'node:selected'
		);

		expect(selectedNodeStyle?.style['border-color']).toBe('#600FED');
		expect(selectedNodeStyle?.style['border-width']).toBe(1.5);
	});
});
