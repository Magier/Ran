import { get, writable } from 'svelte/store';
import type { TTP } from '$lib/api';

type ArmorySearchState = {
	data: TTP[];
	filtered: TTP[];
	search: string;
};

export const filteredArmory = writable<ArmorySearchState>({
	data: [],
	filtered: [],
	search: ''
});

export function setArmory(armory: Map<string, TTP[]>): void {
	const data = [...armory.values()].flat();
	const search = get(filteredArmory).search;
	filteredArmory.set({ data, filtered: filterArmory(data, search), search });
}

export function searchAbilitiesInStore(search: string): void {
	filteredArmory.update((state) => ({
		...state,
		search,
		filtered: filterArmory(state.data, search)
	}));
}

function filterArmory(data: TTP[], search: string): TTP[] {
	const term = search.trim().toLowerCase();
	return term ? data.filter((ttp) => ttp.name.toLowerCase().includes(term)) : data;
}
