import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import { svelteTesting } from '@testing-library/svelte/vite';
import { sveltekit } from '@sveltejs/kit/vite';
import Icons from 'unplugin-icons/vite';
import { defineConfig } from 'vitest/config';

const viteHost = process.env.RAN_VITE_HOST ?? 'localhost';
const requestedPort = Number.parseInt(process.env.RAN_VITE_PORT ?? '5173', 10);
const vitePort =
	Number.isInteger(requestedPort) && requestedPort > 0 && requestedPort <= 65535
		? requestedPort
		: 5173;

export default defineConfig({
	plugins: [
		tailwindcss(),
		sveltekit({
			// Consult https://svelte.dev/docs/kit/integrations
			// for more information about preprocessors
			preprocess: vitePreprocess(),
			adapter: adapter({})
		}),
		Icons({ compiler: 'svelte', autoInstall: true })
	],

	server: {
		host: viteHost,
		port: vitePort,
		strictPort: true,
		// When frontend is served through the Rust proxy, HMR must connect directly
		// to the Vite server because the proxy path only supports plain HTTP forwarding.
		hmr: {
			host: viteHost,
			port: vitePort,
			clientPort: vitePort,
			protocol: 'ws'
		}
	},

	build: {
		minify: 'oxc',
		cssCodeSplit: true, // ensure CSS isn’t bundled into a giant JS chunk
		assetsInlineLimit: 0 // avoid inlining large assets into JS (helps peak memory)
		// SvelteKit configures Rolldown code splitting for the client build.
	},

	test: {
		projects: [
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
