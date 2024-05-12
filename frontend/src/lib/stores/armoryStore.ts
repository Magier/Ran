import { get, writable } from "svelte/store";
import store from "$lib/stores/store";

const groupBy = <T, K extends keyof any>(arr: T[], key: (i: T) => K) =>
    arr.reduce((groups, item) => {
        (groups[key(item)] ||= []).push(item);
        return groups;
    }, {} as Record<K, T[]>);

 function createSearchStore() {
    const {subscribe, set, update} = writable({
        data: [],
        filtered: [],
        search:""
    })

    return {
        subscribe,
        set,
        update
    }
}


export const filteredArmory = createSearchStore();
export const searchAbilitiesInStore = () => {
    debugger
    const searchTerm = filteredArmory.search.toLowerCase() || "";
    store.filtered = store.data.filter((ttp) => {
        return ttp.name.toLowerCase().icludes(searchTerm);
    })
}


store.armory((new_armory: Map<string, TTP[]>) => {
    console.log("~~~~")
    console.log(new_armory)
    console.log("before")
    console.log(filteredArmory)
    // filteredArmory.update(test)
    // armory = new_armory;
    const currentArmory = get(filteredArmory);
    filteredArmory.set({data: new_armory, filtered: new_armory, search:  currentArmory.search})
});