import tailwindcss from '@tailwindcss/vite';
import { svelteTesting } from '@testing-library/svelte/vite';
import { sveltekit } from '@sveltejs/kit/vite';
import Icons from 'unplugin-icons/vite'
import { defineConfig } from 'vite';

export default defineConfig({
	plugins: [tailwindcss(), sveltekit(), Icons({
		compiler: 'svelte',
		autoInstall: true,
	})],

	server: {
		// When frontend is served through the Rust proxy, HMR must connect directly
		// to the Vite server because the proxy path only supports plain HTTP forwarding.
		hmr: {
			host: 'localhost',
			port: 5173,
			clientPort: 5173,
			protocol: 'ws'
		}
	},

	build: {
		minify: 'esbuild',            // much lighter than terser
		cssCodeSplit: true,           // ensure CSS isn’t bundled into a giant JS chunk
		assetsInlineLimit: 0,         // avoid inlining large assets into JS (helps peak memory)
		// Smaller, more numerous chunks are usually easier on memory than one mega vendor chunk
		rollupOptions: {
			output: {
				manualChunks(id) {
				if (id.includes('node_modules')) {
					// group by top-level package name: node_modules/<pkg>/...
					const match = id.toString().split('node_modules/')[1];
					if (!match) return;
					const pkg = match.split('/')[0].startsWith('@')
					? match.split('/').slice(0,2).join('/')
					: match.split('/')[0];
					return `vendor-${pkg}`;
				}
				},
			}
		},
	},

	test: {
		workspace: [
			{
				extends: './vite.config.ts',
				plugins: [svelteTesting()],

				test: {
					name: 'client',
					environment: 'jsdom',
					clearMocks: true,
					include: ['src/**/*.svelte.{test,spec}.{js,ts}'],
					exclude: ['src/lib/server/**'],
					setupFiles: ['./vitest-setup-client.ts']
				}
			},
			{
				extends: './vite.config.ts',

				test: {
					name: 'server',
					environment: 'node',
					include: ['src/**/*.{test,spec}.{js,ts}'],
					exclude: ['src/**/*.svelte.{test,spec}.{js,ts}']
				}
			}
		]
	}
});
