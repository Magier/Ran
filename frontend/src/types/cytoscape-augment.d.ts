// src/types/cytoscape-augment.d.ts
//
// Gaps in cytoscape's own bundled index.d.ts. These are documented, shipped API
// that the type definitions simply omit, so they are declared here rather than
// worked around with casts. Verified against cytoscape 3.33.2; re-check on a
// major bump in case upstream has since typed them.
//
// `show()` / `hide()` are the documented display shortcuts (equivalent to
// setting the `display` style to `element` / `none`). They exist on singulars
// and collections alike but appear nowhere in the bundled declarations. The
// declarations are repeated per interface rather than shared through a base,
// because an interface extending one without adding members is itself a lint
// error.
import 'cytoscape';

declare module 'cytoscape' {
	interface NodeSingular {
		show(): this;
		hide(): this;
	}

	interface EdgeSingular {
		show(): this;
		hide(): this;
	}

	interface NodeCollection {
		show(): this;
		hide(): this;
	}

	interface EdgeCollection {
		show(): this;
		hide(): this;
	}
}
