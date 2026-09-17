export function parseHiddenEdgeTypes(raw: string | null): Set<string> {
	if (!raw) return new Set();

	try {
		const parsed: unknown = JSON.parse(raw);
		if (!Array.isArray(parsed) || parsed.some((value) => typeof value !== 'string')) {
			return new Set();
		}
		return new Set(parsed);
	} catch {
		return new Set();
	}
}

export function toggleHiddenEdgeType(
	hiddenEdgeTypes: ReadonlySet<string>,
	edgeType: string
): Set<string> {
	const next = new Set(hiddenEdgeTypes);
	if (next.has(edgeType)) next.delete(edgeType);
	else next.add(edgeType);
	return next;
}
