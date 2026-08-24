export function defaultPlanName(now = new Date()) {
	return `execution_${now.toISOString().replace(/[:.]/g, '-')}`;
}

export function defaultPlanDescription(now = new Date()) {
	return `Plan exported from campaign execution history on ${now.toISOString()}.`;
}

export function planFilename(name: string) {
	const safeName = name
		.trim()
		.replace(/\.plan\.ya?ml$/i, '')
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-|-$/g, '');
	return `${safeName || 'execution'}.plan.yaml`;
}

export function buildPlanDownload(
	yaml: string,
	name?: string,
	now = new Date(),
	filename?: string
) {
	return {
		data: yaml,
		filename: planFilename(filename || name || defaultPlanName(now)),
		mimeType: 'application/yaml'
	};
}

export function downloadPlan(yaml: string, name?: string) {
	const download = buildPlanDownload(yaml, name);
	const url = URL.createObjectURL(new Blob([download.data], { type: download.mimeType }));
	const anchor = document.createElement('a');
	anchor.href = url;
	anchor.download = download.filename;
	anchor.click();
	URL.revokeObjectURL(url);
}
