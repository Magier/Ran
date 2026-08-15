import type { AttackFlow } from '$lib/api';

export function buildCampaignFlowDownload(flow: AttackFlow, now = new Date()) {
	const timestamp = now.toISOString().replace(/[:.]/g, '-');
	return {
		data: JSON.stringify(flow, null, 2),
		filename: `campaign_${timestamp}.json`,
		mimeType: 'application/json'
	};
}
