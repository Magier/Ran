import { describe, expect, it } from 'vitest';
import cytoscape from 'cytoscape';

import {
	applyCompromisedStyle,
	getGraphStyle,
	getK8sCredentialIcon,
	getUnknownSystemIcon
} from './graph_style';

describe('getK8sCredentialIcon', () => {
	it('selects a contrasting icon for each graph theme', () => {
		expect(getK8sCredentialIcon(true)).toBe('/k8s/account-key-dark.svg');
		expect(getK8sCredentialIcon(false)).toBe('/k8s/account-key-light.svg');
	});

	it('selects a contrasting system icon for each graph theme', () => {
		expect(getUnknownSystemIcon(true)).toBe('/system-dark.svg');
		expect(getUnknownSystemIcon(false)).toBe('/system.svg');
	});

	it('gives a macOS system a themed platform icon, matching uname casing', () => {
		expect(getUnknownSystemIcon(true, 'Darwin')).toBe('/macos-dark.svg');
		expect(getUnknownSystemIcon(false, 'Darwin')).toBe('/macos-light.svg');
		expect(getUnknownSystemIcon(false, 'darwin')).toBe('/macos-light.svg');
	});

	it('falls back to the generic icon for any other or missing os', () => {
		expect(getUnknownSystemIcon(false, 'Linux')).toBe('/system.svg');
		expect(getUnknownSystemIcon(false, undefined)).toBe('/system.svg');
		expect(getUnknownSystemIcon(false, 42)).toBe('/system.svg');
	});

	it('lets the macOS override win over the generic system icon', () => {
		const styles = getGraphStyle(true) as Array<{
			selector: string;
			style: Record<string, unknown>;
		}>;
		const generic = styles.findIndex((rule) => rule.selector === "node[kind='UnknownSystem']");
		const macos = styles.findIndex(
			(rule) => rule.selector === "node[kind='UnknownSystem'][entity.os @= 'darwin']"
		);

		expect(macos).toBeGreaterThan(generic);
		expect(styles[macos].style['background-image']).toBe('/macos-dark.svg');
	});

	it('draws a generic system as a rectangle rather than the k8s heptagon', () => {
		const styles = getGraphStyle(false) as Array<{
			selector: string;
			style: Record<string, unknown>;
		}>;
		const heptagonIndex = styles.findIndex((rule) => rule.selector === 'node[?kind]');
		const systemIndex = styles.findIndex((rule) => rule.selector === "node[kind='UnknownSystem']");

		expect(systemIndex).toBeGreaterThan(heptagonIndex);
		expect(styles[systemIndex].style.shape).toBe('rectangle');
		expect(styles[systemIndex].style['background-image']).toBe('/system.svg');
	});

	it('has no leftover pre-rename System selectors', () => {
		const styles = getGraphStyle(false) as Array<{ selector: string }>;

		expect(
			styles.filter((rule) => /\[kind\s*=\s*['"]System['"]\]/.test(rule.selector))
		).toHaveLength(0);
		expect(styles.filter((rule) => rule.selector.includes('[^kind]'))).toHaveLength(0);
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
		const baseEdgeStyle = style.find((rule: { selector: string }) => rule.selector === 'edge');
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

	it('hides a deployment icon while its compound is expanded', () => {
		const styles = getGraphStyle() as Array<{ selector: string; style: Record<string, unknown> }>;
		const deploymentIcon = styles.find((rule) => rule.selector === "node[kind='Deployment']");
		const expandedCompound = styles.find((rule) =>
			rule.selector.includes("node[kind='Deployment']:parent")
		);

		expect(deploymentIcon?.style['background-image']).toEqual(['/k8s/deploy.svg']);
		expect(expandedCompound?.style['background-image']).toBe('none');
	});

	it('uses the available assets for Kubernetes workload kinds', () => {
		const styles = getGraphStyle();
		for (const [kind, icon] of [
			['DaemonSet', '/k8s/ds.svg'],
			['ReplicaSet', '/k8s/rs.svg'],
			['StatefulSet', '/k8s/sts.svg']
		]) {
			expect(
				styles.find((rule) => rule.selector === `node[kind='${kind}']`)?.style['background-image']
			).toEqual([icon]);
		}
	});

	it('keeps the icon on a compromised collapsed deployment and hides it when expanded', () => {
		const cy = cytoscape({
			headless: true,
			styleEnabled: true,
			style: getGraphStyle() as cytoscape.StylesheetJson,
			elements: [
				{
					data: {
						id: 'deployment',
						name: 'deployment',
						kind: 'Deployment',
						compromised: true,
						containsCompromised: true
					}
				},
				{ data: { id: 'pod', name: 'pod', kind: 'Pod', parent: 'deployment' } }
			]
		});
		const deployment = cy.getElementById('deployment');
		const imageLayers = () =>
			(deployment as unknown as { pstyle(name: string): { value: string[] } }).pstyle(
				'background-image'
			).value;

		deployment.addClass('cy-expand-collapse-collapsed-node');
		cy.getElementById('pod').remove();
		cy.batch(() => {
			for (let i = 0; i < 5; i++) applyCompromisedStyle(cy);
		});
		expect(imageLayers()).toHaveLength(2);
		expect(imageLayers()[0]).toBe('/k8s/deploy.svg');
		expect(imageLayers()[1]).toMatch(/^data:image\/svg\+xml,/);
		const tint = imageLayers()[1];
		deployment.style('background-image', [`/k8s/deploy.svg ${tint}`, tint, tint]);
		applyCompromisedStyle(cy);
		expect(imageLayers()).toEqual(['/k8s/deploy.svg', tint]);

		cy.add({ data: { id: 'pod', name: 'pod', kind: 'Pod', parent: 'deployment' } });
		deployment.removeClass('cy-expand-collapse-collapsed-node');
		cy.batch(() => {
			for (let i = 0; i < 5; i++) applyCompromisedStyle(cy);
		});
		expect(imageLayers()).toHaveLength(1);
		expect(imageLayers()[0]).toMatch(/^data:image\/svg\+xml,/);

		deployment.addClass('cy-expand-collapse-collapsed-node');
		cy.getElementById('pod').remove();
		cy.batch(() => {
			for (let i = 0; i < 5; i++) applyCompromisedStyle(cy);
		});
		expect(imageLayers()).toHaveLength(2);
		expect(imageLayers()[0]).toBe('/k8s/deploy.svg');
		cy.destroy();
	});

	it.each([
		['UnknownSystem', { os: 'Darwin' }, '/macos-dark.svg'],
		['K8sCredential', undefined, '/k8s/account-key-dark.svg']
	])('keeps the current-theme icon when %s is no longer compromised', (kind, entity, darkIcon) => {
		const cy = cytoscape({
			headless: true,
			styleEnabled: true,
			style: getGraphStyle(false) as cytoscape.StylesheetJson,
			elements: [{ data: { id: 'target', name: 'target', kind, entity, compromised: true } }]
		});
		const node = cy.getElementById('target');

		applyCompromisedStyle(cy);
		// graph.svelte applies the new theme icon as an inline style.
		node.style('background-image', darkIcon);
		applyCompromisedStyle(cy);
		node.data('compromised', false);
		applyCompromisedStyle(cy);

		expect(node.style('background-image')).toBe(darkIcon);
		cy.destroy();
	});

	it('lets custom resource icons inherit the shared node sizing', () => {
		const customResource = getGraphStyle().find(
			(rule: { selector: string }) => rule.selector === 'node[?customResource]'
		);

		expect(customResource?.style['background-image']).toBe('/k8s/crd.svg');
		expect(customResource?.style.width).toBeUndefined();
		expect(customResource?.style.height).toBeUndefined();
	});
});
