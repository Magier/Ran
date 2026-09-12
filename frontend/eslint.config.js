import prettier from 'eslint-config-prettier';
import js from '@eslint/js';
import { includeIgnoreFile } from '@eslint/compat';
import svelte from 'eslint-plugin-svelte';
import globals from 'globals';
import { fileURLToPath } from 'node:url';
import ts from 'typescript-eslint';
import svelteConfig from './svelte.config.js';
const gitignorePath = fileURLToPath(new URL('./.gitignore', import.meta.url));

export default ts.config(
	includeIgnoreFile(gitignorePath),
	js.configs.recommended,
	...ts.configs.recommended,
	...svelte.configs.recommended,
	prettier,
	...svelte.configs.prettier,
	{
		languageOptions: {
			globals: {
				...globals.browser,
				...globals.node
			}
		},
		rules: {
			// A leading underscore marks a binding that exists for its side effect,
			// not its value. The common case is a $effect dependency tracker:
			// reading the value is the point, so the read must not be deleted.
			'@typescript-eslint/no-unused-vars': [
				'error',
				{
					argsIgnorePattern: '^_',
					varsIgnorePattern: '^_',
					caughtErrorsIgnorePattern: '^_'
				}
			]
		}
	},
	{
		// Tech debt from before frontend linting was enforced, tracked in
		// https://github.com/Magier/Ran/issues/52. Warnings so CI gates every
		// other rule at error while these are worked down; each one goes back
		// to 'error' as its area is cleaned up.
		//
		//   @typescript-eslint/no-explicit-any    cytoscape and its plugins
		//   svelte/require-each-key               {#each} blocks without a key
		//   svelte/prefer-svelte-reactivity       plain Map/Set in reactive state
		rules: {
			'@typescript-eslint/no-explicit-any': 'warn',
			'svelte/require-each-key': 'warn',
			'svelte/prefer-svelte-reactivity': 'warn'
		}
	},
	{
		// The cytoscape modules are fully typed (#52 follow-up): the plugins have
		// hand-written declarations in src/types, so `any` is an error here even
		// while the rest of the tree still carries it as a warning. Adding a file
		// to this list means committing to keeping it clean.
		files: [
			'src/types/**/*.d.ts',
			'src/routes/components/graph.svelte',
			'src/routes/components/graph_edges.ts',
			'src/routes/components/graph_edges.svelte.test.ts',
			'src/routes/components/graph_style.ts',
			'src/routes/components/graph_node_selector.svelte',
			'src/routes/components/elk_layout.ts'
		],
		rules: {
			'@typescript-eslint/no-explicit-any': 'error'
		}
	},
	{
		files: ['**/*.svelte', '**/*.svelte.ts', '**/*.svelte.js'],
		ignores: ['eslint.config.js', 'svelte.config.js'],

		languageOptions: {
			parserOptions: {
				projectService: true,
				extraFileExtensions: ['.svelte'],
				parser: ts.parser,
				svelteConfig
			}
		}
	}
);
