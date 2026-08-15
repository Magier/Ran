import { describe, expect, it } from 'vitest';

import { buildCampaignFlowDownload } from './campaignFlow';

describe('buildCampaignFlowDownload', () => {
	it('serializes Ran campaign flow JSON with a portable filename', () => {
		const flow = { steps: [], edges: [] };
		const download = buildCampaignFlowDownload(flow, new Date('2026-08-15T09:10:11.123Z'));

		expect(download).toEqual({
			data: JSON.stringify(flow, null, 2),
			filename: 'campaign_2026-08-15T09-10-11-123Z.json',
			mimeType: 'application/json'
		});
	});
});
