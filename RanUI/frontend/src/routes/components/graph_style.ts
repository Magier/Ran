// import IconListener from '~icons/game-icons/fishing-net';
// import function to get icon data from iconify
// import { getIcon } from '@iconify/svelte';
// import { listIcons } from '@iconify/svelte';


// function getIconData(iconName) {
// 	const a = IconListener
// 	debugger
// 	const icon = getIcon('cil:paper-plane');
// 	const data = icon.body;
// 	// return getIcon('').toSvg();
// 	return 'data:image/svg+xml;utf8,' + encodeURIComponent(data);
// }


export const layout = {
	name: 'dagre',
	rankDir: 'LR',
	align: 'DR',
	nodeSep: 50,        // min distance between nodes on same rank
	edgeSep: 15,
	rankSep: 75,
	padding: 30,
	// animate: true
};

const kind_svg_map = {
	Ingress: 'ing.svg',
	Pod: 'pod.svg',
	Container: 'crio.svg',
	Daemonset: 'daemontset.svg',
	Deployment: 'deploy.svg',
	AbstractWorkload: 'deploy.svg',
	ControlPlane: 'control-plane.svg',
	ClusterNode: 'node.svg',
	Node: 'node.svg',
	Role: 'role.svg',
	ClusterRole: 'c-role.svg',
	Service: 'svc.svg',
	ConfigMap: 'cm.svg',
	CronJob: 'cronjob.svg',
	Group: 'group.svg',
	RoleBinding: 'rb.svg',
	ClusterRoleBinding: 'crb.svg',
	Secret: 'secret.svg',
	ServiceAccount: 'sa.svg',
	Statefulset: 'sts.svg',
	User: 'user.svg',
	Volume: 'vol.svg',
	KubeApiServer: 'api.svg',
	MicroService: '{pod_unlabeled.svg',

	Cluster: '{k8s.svg',
	Namespace: '{ns.svg'
};

function mapKindIcons(obj: Object) {
	let new_entries = Object.entries(obj).map(([kind, img]) => {
		let [icon, isCompound] = img.startsWith('{') ? [img.substring(1), true] : [img, false];
		let kind_selector = `node[kind='${kind}']`;
		let selector = isCompound
			? `${kind_selector}.abstract, ${kind_selector}:childless`
			: kind_selector;
		return {
			selector: selector,
			style: {
				'background-image': [`/k8s/${icon}`]//, 'data:image/svg+xml;utf8,<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect width="100%" height="100%" fill="red" fill-opacity="0.4"/></svg>'],
			}
		};
	});

	return new_entries;
};


export function getGraphStyle() {
	const style = getComputedStyle(document.body)
	function css(name: string) {
		if (name[0] != '-') name = '--' + name //allow passing with or without --
		const rgb = style.getPropertyValue(name).replaceAll(" ", ", ");
		return `rgb(${rgb})`;
	}
	const primary = css('color-primary-500')
	// const textColor = css('white')
	const textColor = 'white'
	// const textColor = css('text-primary-contrast-200-800')
	const selectedTextColor = css('white')
	const surface = css('color-surface-500');


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
				'font-size': '12',
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
			selector: '[kind="Namespace"]:parent',
			style: {
				'border-color': '#326CE5',
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
				'border-width': 2
				// 'background-image': null
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
			selector: "node[kind='Pod'][!entity.isRunning]",
			style: {
				width: '20',
				height: '20',
				'background-image': '/k8s/pod_transparent.svg',
			}
		},
		{
			selector: "node[?compromised]",
			style: {
				// 'color': '#600FED',
				'border-color': '#600FED',
				'border-width': 2,
				'background-color': 'red',
				// 'background-image': [
				// 	'k8s/pod.svg',
				// 	'data:image/svg+xml;utf8,<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect width="100%" height="100%" fill="red" fill-opacity="0.4"/></svg>'],
				// 'background-opacity': 0.4, // Adjust for desired tint strength
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
			selector: "node[kind='Namespace'][entity.EnforcedPSS *= 'baseline']",
			style: {
				'background-color': 'orange',
				'border-color': 'orange'
			}
		},
		{
			selector: "node[kind='Namespace'][entity.EnforcedPSS *= 'restricted']",
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
				// color: 'gray',
				'edge-text-rotation': 'autorotate',
				// 'text-background-color': 'none',
				'text-background-opacity': '0',
				'text-background-padding': '3',
				'font-size': '10',
				'text-margin-y': '-10px',
				width: '1',
				color: 'white',
				'target-arrow-shape': 'triangle',
				content: 'data(name)'
				// 'line-color': 'gray',
				// 'target-arrow-color': 'gray'
				// 'font-weight': 'bold'
			}
		},
		{
			selector: 'edge[name="controls"]',
			style: {
				color: textColor,
				'line-color': primary,
				'target-arrow-color': primary,
				width: '2'
			}
		},
		{
			selector: 'edge[name="references"]',
			style: {
				color: 'gray',
				'line-color': 'gray',
				'target-arrow-color': 'gray',
				'line-style': 'dotted',
				'font-size': 7
			}
		},
		{
			selector: 'edge[name="runs-on"]',
			style: {
				color: 'gray',
				'line-color': 'gray',
				'target-arrow-color': 'gray',
				'line-style': 'dotted',
				'font-size': 7
			}
		},
		{
			selector: "edge[relation='routes']",
			style: {
				content: `data(port)`
			}
		},
		{
			selector: "edge[relation='can-reach']",
			style: {
				width: 0.5,
				'line-style': 'dashed',
				color: 'gray',
				'line-color': 'gray',
				'target-arrow-color': 'gray'
			}
		},
		{
			selector: 'edge[?proxyEdge]',
			style: {
				'line-color': 'green',
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
	return mapKindIcons(kind_svg_map).concat(graph_style);
}