// import { getContext } from 'svelte';
// import { type ToastContext } from '@skeletonlabs/skeleton-svelte';
import * as toast from '@zag-js/toast';
import { createToaster } from '@skeletonlabs/skeleton-svelte';


export const toaster: toast.Store<any> = createToaster({
    placement: 'bottom-end',
});

// export const toaster: ToastContext = getContext('toast');

type ToastType = 'info' | 'error' | 'success' | undefined;
export function showToast(title: string, description: string, toastType: ToastType): string {
    console.log('Showing toast', title, description, toastType);
    return toaster.create({ title: title, description: description, type: toastType, duration: 5000 });
}
