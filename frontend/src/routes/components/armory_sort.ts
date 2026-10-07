import type { TTP } from '#lib/api/index.js';

export function sortTtpsByName(ttps: TTP[]): TTP[] {
	return [...ttps].sort((a, b) => {
		const nameOrder = a.name.localeCompare(b.name, 'en', { sensitivity: 'base' });
		return nameOrder || a.id.localeCompare(b.id, 'en');
	});
}
