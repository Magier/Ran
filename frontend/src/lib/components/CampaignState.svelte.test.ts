import { describe, expect, it, vi } from 'vitest';
import type { CampaignState as CampaignSnapshot, Graph } from '#lib/api/index.js';
import type { RanAPI } from '#lib/ran_api.js';
import { CampaignState, executionFailureMessage } from './CampaignState.svelte';

function fakeApi(graph: Promise<Graph>, snapshot: Promise<CampaignSnapshot>): RanAPI {
	return {
		on: vi.fn(() => vi.fn()),
		onConnectionStateChange: vi.fn(() => vi.fn()),
		connect: vi.fn().mockResolvedValue(undefined),
		GetUiConfig: vi.fn().mockResolvedValue({ namespaces: { defaultVisible: true, rules: [] } }),
		GetKubetierCatalog: vi.fn().mockResolvedValue(null),
		GetArmory: vi.fn().mockResolvedValue([]),
		GetGraph: vi.fn(() => graph),
		GetCampaignState: vi.fn(() => snapshot)
	} as unknown as RanAPI;
}

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

describe('CampaignState initialization', () => {
	it('does not become ready before graph and campaign state form one snapshot', async () => {
		let resolveGraph!: (graph: Graph) => void;
		const graphPromise = new Promise<Graph>((resolve) => {
			resolveGraph = resolve;
		});
		const snapshot = Promise.resolve({
			entities: {
				'c2/ran': { id: 'c2/ran', name: 'Ran', kind: 'C2' }
			},
			entityAliases: {},
			relations: []
		} satisfies CampaignSnapshot);
		const campaign = new CampaignState();
		campaign.api = fakeApi(graphPromise, snapshot);

		const initialized = campaign.init();
		await snapshot;
		await Promise.resolve();

		expect(campaign.isReady()).toBe(false);
		expect(campaign.graph).toEqual({ nodes: [], edges: [], rootNodeId: '' });

		resolveGraph({ nodes: [], edges: [], rootNodeId: 'c2/ran' });
		await initialized;

		expect(campaign.isReady()).toBe(true);
		expect(campaign.entities.map((entity) => entity.id)).toEqual(['c2/ran']);
	});

	it('treats an empty campaign snapshot as loaded', async () => {
		const graph = Promise.resolve({ nodes: [], edges: [], rootNodeId: '' });
		const snapshot = Promise.resolve({
			entities: {},
			entityAliases: {},
			relations: []
		} satisfies CampaignSnapshot);
		const campaign = new CampaignState();
		campaign.api = fakeApi(graph, snapshot);

		await campaign.init();

		expect(campaign.isReady()).toBe(true);
	});
});
