import { browser } from '$app/env';

const AUTO_OPEN_READ_FILES_KEY = 'ran.autoOpenReadFiles';

export class UiPreferences {
	autoOpenReadFiles = $state(true);
	private storage: Pick<Storage, 'getItem' | 'setItem'> | undefined;

	constructor(storage: Pick<Storage, 'getItem' | 'setItem'> | undefined = undefined) {
		this.storage = storage ?? (browser ? localStorage : undefined);
		if (this.storage) {
			this.autoOpenReadFiles = this.storage.getItem(AUTO_OPEN_READ_FILES_KEY) !== 'false';
		}
	}

	setAutoOpenReadFiles(enabled: boolean): void {
		this.autoOpenReadFiles = enabled;
		this.storage?.setItem(AUTO_OPEN_READ_FILES_KEY, String(enabled));
	}
}

export const uiPreferences = new UiPreferences();
