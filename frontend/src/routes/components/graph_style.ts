import type cytoscape from "cytoscape";
import { WORKLOAD_KINDS } from './workload_compounds';

const KIND_SVG_MAP = {
	AppService: 'k8s/ep.svg',
	Ingress: 'k8s/ing.svg',
	Pod: 'k8s/pod.svg',
	Container: 'k8s/crio.svg',
	Daemonset: 'k8s/daemontset.svg',
	Deployment: 'k8s/deploy.svg',
	AbstractWorkload: 'k8s/deploy.svg',
	ControlPlane: 'k8s/control-plane.svg',
	ClusterNode: 'k8s/node.svg',
	Node: 'k8s/node.svg',
	Role: 'k8s/role.svg',
	ClusterRole: 'k8s/c-role.svg',
	Service: 'k8s/svc.svg',
	ConfigMap: 'k8s/cm.svg',
	CronJob: 'k8s/cronjob.svg',
	Job: 'k8s/job.svg',
	Group: 'k8s/group.svg',
	RoleBinding: 'k8s/rb.svg',
	ClusterRoleBinding: 'k8s/crb.svg',
	Secret: 'k8s/secret.svg',
	ServiceAccount: 'k8s/sa.svg',
	Statefulset: 'k8s/sts.svg',
	User: 'k8s/user.svg',
	Volume: 'k8s/vol.svg',
	KubeApiServer: 'k8s/api.svg',
	MicroService: 'k8s/pod_unlabeled.svg',
	GCPBucket: 'gcp/storage.svg',
	GCPServiceAccount: 'gcp/iam.svg',
	GCPServiceAccountToken: 'gcp/iam.svg',
	MetadataServer: 'gcp/compute_engine.svg',
	GCPMetadataServer: 'gcp/compute_engine.svg',
	UnknownSystem: 'system-dark.svg',

	Cluster: 'k8s/k8s.svg',
	Namespace: 'k8s/ns.svg'
};

// Compounds show an icon only while collapsed. Once a compound is expanded,
// its children provide the visual identity for the group.
const COMPOUND_KINDS = new Set([
	...WORKLOAD_KINDS,
	'CronJob',
	'Cluster',
	'Namespace',
	'MicroService'
]);
const expandedCompoundSelector = [...COMPOUND_KINDS]
	.map((kind) => `node[kind='${kind}']:parent`)
	.join(', ');

export function getK8sCredentialIcon(isDark: boolean): string {
	return isDark ? '/k8s/account-key-dark.svg' : '/k8s/account-key-light.svg';
}

function mapKindIcons(obj: Record<string, string>) {
	return Object.entries(obj).map(([kind, icon]) => {
		return {
			selector: `node[kind='${kind}']`,
			style: {
				'background-image': [`/${icon}`]//, 'data:image/svg+xml;utf8,<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect width="100%" height="100%" fill="red" fill-opacity="0.4"/></svg>'],
			}
		};
	});
}

export function getGraphStyle(isDark: boolean = false) {
	// Cytoscape does not accept the Mona theme's native oklch() color value.
	const primary = '#600FED';
	const textColor = isDark ? 'white' : 'black';
	const selectedTextColor = textColor;

	const graph_style = [
		{
			selector: 'node',
			style: {
				width: '20',
				height: '20',
				color: textColor,
				'background-fit': 'cover',
				'background-clip': 'none',
				'background-opacity': 0,
				// 'text-opacity': '0.4',
				content: `data(name)`,
				'font-size': '9',
				// 'font-weight': 'bold',
				'text-valign': 'bottom',
				'text-wrap': 'wrap',
				'text-max-width': '80',
				'text-margin-y': '5px'
			}
		},
		{
			selector: ':parent',
			style: {
				shape: 'round-rectangle',
				'border-style': 'dashed',
				'background-color': 'steelblue',
				'background-opacity': 0.2
			}
		},
		{
			selector: expandedCompoundSelector,
			style: {
				'background-image': 'none'
			}
		},
		{
			selector: '[kind="Namespace"]:parent',
			style: {
				'border-color': '#326CE5',
			}
		},
		{
			selector: 'node[kind="OperatorHost"]:parent',
			style: {
				'border-style': 'solid',
				'border-width': 1,
				'border-color': isDark ? '#64748b' : '#94a3b8',
				'background-color': isDark ? '#334155' : '#e2e8f0',
				'background-opacity': 0.16,
				'text-valign': 'bottom',
				'text-halign': 'center',
				'text-margin-x': 0,
				'text-margin-y': 5
			}
		},
		{
			selector: 'node:selected',
			style: {
				'background-color': primary,
				color: selectedTextColor,
				'border-color': primary,
				'line-color': primary,
				'target-arrow-color': primary,
				'border-width': 1.5
				// 'background-image': null
			}
		},
		{
			selector: '.context-dimmed',
			style: {
				opacity: 0.48,
				'text-opacity': 0.38
			}
		},
		{
			selector: 'node[?scenarioProvided]',
			style: {
				'border-width': 3,
				'border-style': 'double',
				'border-color': '#f59e0b'
			}
		},
		{
			selector: "node[kind='Service']",
			style: {
				width: '15',
				height: '15',
				'font-size': 8
			}
		},
		{
			selector: "node[kind='Pod']",
			style: {
				width: '30',
				height: '30',
			}
		},
		{
			selector: "node[kind='Pod'][!isRunning]",
			style: {
				width: '20',
				height: '20',
				'background-image': '/k8s/pod_transparent.svg',
			}
		},

		{
			selector: "node[kind='Node']",
			style: {
				width: '30',
				height: '30',
			}
		},
		{
			selector: "node[name='Adversary']",
			style: {
				'background-opacity': 0,
				shape: 'round-rectangle',
				'background-image': '/adversary-dark.svg'
			}
		},
		{
			selector: "node[name='Adversary']:selected",
			style: {}
		},
		{
			selector: 'node[?kind]',
			style: {
				shape: 'heptagon',
				// 'background-color': 'steelblue'
			}
		},
		{
			selector: "node[kind='K8sCredential']",
			style: {
				width: '30',
				height: '20',
				shape: 'rectangle',
				'background-image': getK8sCredentialIcon(isDark),
				'background-fit': 'contain',
				'background-opacity': 0,
				'border-position': 'outside'
			}
		},
		{
			selector: 'node[name="Ran"]',
			style: {
				width: '30',
				height: '30',
				'background-image': '/Ran.svg'
			}
		},
		{
			selector: 'node[kind="C2"][name!="Ran"]',
			style: {
				'background-opacity': 0,
				shape: 'round-rectangle',
				'background-image': '/c2-dark.svg'
			}
		},
		{
			selector: 'node[kind="Listener"]',
			style: {
				shape: 'rectangle',
				'background-opacity': 0,
				'background-image': '/listener.svg',
			}
		},
		{
			selector: 'node[kind="System"]',
			style: {
				'background-image': '/system.svg',
				'background-opacity': 0,
				'background-fit': 'contain'
			}
		},
		{
			selector: "node[^kind][os='macos']",
			style: {
				'background-image': '/macos.svg',
				'background-opacity': 0,
				'background-fit': 'contain'
			}
		},
		{
			selector: "node[^kind][os='linux']",
			style: {
				'background-image': '/system.svg',
				'background-opacity': 0,
				'background-fit': 'contain'
			}
		},
		{
			selector: "node[kind='Namespace'][entity.enforcedPSS *= 'baseline']",
			style: {
				'background-color': 'orange',
				'border-color': 'orange'
			}
		},
		{
			selector: "node[kind='Namespace'][entity.enforcedPSS *= 'restricted']",
			style: {
				'background-color': 'red',
				'border-color': 'red'
			}
		},
		{
			selector: 'node[kind="Session"]',
			style: {
				shape: 'rectangle',
				'background-opacity': 0,
				'background-image': '/session.svg',
			}
		},
		// {
		// 	selector: 'node[kind="property"]',
		// 	style: {
		// 		'background-color': 'red',
		// 		width: '10',
		// 		height: '10'
		// 	}
		// },
		// {
		// 	selector: 'node[!kind]',
		// 	style: {
		// 		width: '10',
		// 		height: '10'
		// 	}
		// },
		{
			selector: `node.abstract`,
			style: {
				'border-width': 0,
				'background-opacity': 0,
				'background-image-containment': 'inside',
				'background-fit': 'contain',
				shape: 'heptagon'
			}
		},
		{
			selector: `node[!kind].abstract`,
			style: {
				'background-image': 'component.svg',
				shape: 'heptagon'
			}
		},
		{
			selector: 'node[?highlighted]',
			style: {
				'border-width': 3,
				'border-color': 'green',
				width: '40',
				height: '40',
			}
		},
		// {
		// 	selector: 'node[?hidden]',
		// 	style: { display: 'none' }
		// },
		// { selector: '.hidden', style: { visibility: 'hidden' } },
		// {
		// 	selector: 'node[?compromised]',
		// 	style: {
		// 		'background-color': 'darkred',
		// 		'border-width': 2
		// 	}
		// },
		// {
		// 	selector: 'node[?at-risk]',
		// 	style: {
		// 		'background-color': 'darkorange'
		// 	}
		// },
		{
			selector: 'edge',
			style: {
				'curve-style': 'bezier',
				// 'curve-style': 'taxi',
				'edge-text-rotation': 'autorotate',
				// 'text-background-color': 'none',
				'text-background-opacity': '0',
				'text-background-padding': '3',
				'font-size': '10',
				'text-margin-y': '-10px',
				width: '1',
				color: textColor,
				'target-arrow-shape': 'triangle',
				content: '',
				'line-color': textColor,
				'target-arrow-color': textColor
				// 'font-weight': 'bold'
			}
		},
		{
			selector: 'edge.hovered, edge:selected',
			style: {
				content: 'data(name)'
			}
		},
		{
			selector: 'edge[?scenarioProvided]',
			style: {
				'line-style': 'dashed',
				'line-color': '#f59e0b',
				'target-arrow-color': '#f59e0b'
			}
		},
		{
			// Style for meta-edges (grouped edges from collapsed nodes)
			selector: 'edge[?isMetaEdge]',
			style: {
				'width': '3',
				'font-weight': 'bold',
				'font-size': '11',
				color: primary,
				'line-color': primary,
				'target-arrow-color': primary,
				'line-style': 'solid',
				'curve-style': 'bezier'
			}
		},
		{
			selector: 'edge[name="controls"]',
			style: {
				color: primary,
				'line-color': primary,
				'target-arrow-color': primary,
				width: '2'
			}
		},
		{
			// Informational edges: subdued dotted style (driven by data attribute set in graph.svelte)
			selector: 'edge[?informational]',
			style: {
				color: 'gray',
				'line-color': 'gray',
				'target-arrow-color': 'gray',
				'line-style': 'dotted',
				'font-size': 7
			}
		},
		{
			selector: "edge[relation='routes'].hovered, edge[relation='routes']:selected",
			style: {
				content: `data(port)`
			}
		},
		{
			selector: 'edge[?proxyEdge]',
			style: {
				color: 'green',
				'line-color': 'green',
				'target-arrow-color': 'green',
				'line-style': 'dashed'
			}
		},
		// {
		// 	selector: 'edge[?reachable]',
		// 	style: {
		// 		'line-color': 'red',
		// 		'target-arrow-color': 'red',
		// 		width: 4
		// 	}
		// },
		{
			selector: 'edge:selected',
			style: {
				color: 'darkred',
				'line-color': 'darkred',
				'target-arrow-color': 'darkred',
				'border-width': 2
			}
		},

		{
			selector: "edge[relation='has-property']",
			style: {
				display: 'none'
			}
		}
	];
	return [...mapKindIcons(KIND_SVG_MAP), ...graph_style] as any;
}

const redTintSvg = 'data:image/svg+xml,' + encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect width="100%" height="100%" fill="red" fill-opacity="0.4"/></svg>');

export function applyCompromisedStyle(cy: cytoscape.Core) {
	cy.nodes().forEach(n => {
		const shouldTint = Boolean(n.data('compromised')) || (
			n.hasClass('cy-expand-collapse-collapsed-node') && Boolean(n.data('containsCompromised'))
		);
		const img = n.style('background-image');
		const hasTint = typeof img === 'string' && img.includes(redTintSvg);

		if (!shouldTint && hasTint) {
			const layers = img.split(',').map((l: string) => l.trim()).filter((l: string) => l !== redTintSvg);
			n.removeStyle('background-color');
			n.removeStyle('background-opacity');
			n.style({
				'color': '',
				'background-image': layers.length > 0 ? layers.join(', ') : 'none',
			});
		} else if (shouldTint && !hasTint) {
			n.style({
				'background-color': 'red',
				'background-image': [img, redTintSvg],
				'background-opacity': 0.4,
			});
		}
	});
}
