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

	it('shows edge labels on interaction without increasing their width', () => {
		const style = getGraphStyle(false);
		const baseEdgeStyle = style.find(
			(rule: { selector: string }) => rule.selector === 'edge'
		);
		const hoveredEdgeStyle = style.find(
			(rule: { selector: string }) => rule.selector === 'edge.hovered, edge:selected'
		);

		expect(baseEdgeStyle?.style.content).toBe('');
		expect(baseEdgeStyle?.style.width).toBe('1');
		expect(hoveredEdgeStyle?.style.content).toBe('data(name)');
		expect(hoveredEdgeStyle?.style.width).toBeUndefined();
	});

	it('provides a low-opacity style for graph context outside the selection', () => {
		const dimmedStyle = getGraphStyle(false).find(
			(rule: { selector: string }) => rule.selector === '.context-dimmed'
		);

		expect(dimmedStyle?.style.opacity).toBe(0.48);
		expect(dimmedStyle?.style['text-opacity']).toBe(0.38);
	});
});
