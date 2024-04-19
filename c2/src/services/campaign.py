import base64
from collections import defaultdict
from functools import lru_cache
from itertools import groupby
import json
from typing import Tuple
import domain
from domain import commands, events
from domain.armory import get_armory
from domain.entities import (
    C2,
    AccessLevel,
    ApiServer,
    Entity,
    ServiceAccountToken,
    Namespace,
    Relation,
    Service,
    ServiceAccount,
    System,
    Pod,
    Topology,
)
from domain.events import Event, EventType
from domain import events
from services.messagebus import MessageBus


class Campaign:
    def __init__(self):
        self.msg_bus = None
        self.entities: dict[str, Entity] = {"c2": C2(name="Adversary")}
        self.loot: list = []
        self.armory = get_armory()
        self.relations: list[Relation] = []

    async def get_topology(self) -> Topology:
        return Topology(entities=list(self.entities.values()), relations=self.relations)

    async def register(self, msg_bus: MessageBus) -> None:
        self.msg_bus = msg_bus

        msg_bus.register_event_handler(events.SessionConnected, self.add_session)
        msg_bus.register_event_handler(events.SessionDisconnected, self.remove_session)
        msg_bus.register_event_handler(events.ServiceAccountTokenExtracted, self.handle_extracted_serviceaccount_token)
        msg_bus.register_event_handler(
            events.EnvironmentVariablesExtracted, self.analyze_extracted_environment_variables
        )
        msg_bus.register_event_handler(events.TokenCapabilitiesQueried, self.analyze_token_permissions)
        msg_bus.register_event_handler(events.SystemBinariesListed, self.analyze_system_binaries)
        msg_bus.register_event_handler(events.NewFacts, self.add_new_facts)
        msg_bus.register_command_handler(commands.ResetCampaign, self.reset_campaign)
        msg_bus.register_command_handler(commands.DeleteEntities, self.delete_entities)
        msg_bus.register_command_handler(commands.PrepareTTP, self.prepare_ttp)

    async def reset_campaign(self, cmd: commands.ResetCampaign | None = None) -> Event | None:
        self.entities = {"c2": C2(name="Adversary")}
        self.relations = []
        self.loot = []
        return events.UiEvent(type=EventType.Topology, data=Topology(entities=list(self.entities.values())))

    async def add_new_facts(self, event: events.NewFacts) -> Event | None:
        # TODO build relations with entities and not IDs; use IDs only when serializing to UI
        self.loot += event.assets
        for e in event.entities:
            if e.id in self.entities:
                e = merge_entities(self.entities[e.id], e)
            self.entities[e.id] = e

        # TODO add any implicitely configured namespaces

        self.relations += event.relations
        return events.UiEvent(
            type=EventType.Topology, data=Topology(entities=self.entities.values(), relations=self.relations)
        )

    async def add_session(self, event: events.SessionConnected) -> Event | None:
        try:
            new_system = event.system
            new_system.access_level = AccessLevel.UserExecute  # a new session means a full reverse shell
            self.entities[new_system.id] = new_system
            back_channel = Relation(name=f"{event.system.transport} channel", source=self.entities["c2"].id, destination=new_system.id)
            self.relations.append(back_channel)
            ev = events.UiEvent(
                type=EventType.Topology, data=Topology(entities=self.entities.values(), relations=self.relations)
            )
            return ev
        except Exception as exc:
            print("ERROR adding a session: " + str(exc))
        return None

    async def remove_session(self, event: events.SessionDisconnected) -> Event | None:
        """When a C2 session is disconnected degrade the state of the underlying entity to just 'known'

        :param event: the event of the disconnected C2 session
        :return: if an entity is effected, send an statechange event
        """
        try:
            # TODO finding the session based on the system name can be brittle, as only the latest identified name is tracked; maybe keep track of previous names, e.g. before extracting info from SA token?
            system = next((e for e in self.entities.values() if event.system_name in ([e.name] + e.aliases)), None)
            if system is None:
                print("No system '{event.system_name}' (for session id '{session_id}') found! No session was removed!")
            else:
                return events.SystemStateChanged(system_id=system.id, state=domain.SystemState.Known)
        except Exception as exc:
            print("ERROR removing session '{session.Hostname}': " + str(exc))
        return None

    async def delete_entities(self, cmd: commands.DeleteEntities) -> Event | None:
        for entity_id in cmd.entities:
            # remove any relation to the deleted entities
            self.relations = [r for r in self.relations if not (r.source == entity_id or r.destination == entity_id)]
            self.entities.pop(entity_id)
        ev = events.UiEvent(
            type=EventType.Topology, data=Topology(entities=self.entities.values(), relations=self.relations)
        )
        return ev
    
    async def prepare_ttp(self, cmd: commands.PrepareTTP) -> commands.Command | None:

        ttp = self.armory.get_ttp_by_id(cmd.ttp_id)
        if ttp is None:
            raise ValueError(f"No TTP with the value '{cmd.ttp_id}' registered in the Armory")

        if cmd.system_id is None:
            pass
            # TODO try to infer best target for the action 

        return commands.ExecuteTTP(
            name=cmd.name,
            system_id = cmd.system_id,
            ttp=ttp,
            target=cmd.target,
            technique=cmd.technique,
            params=cmd.params,
        )

    async def handle_extracted_serviceaccount_token(
        self, event: events.ServiceAccountTokenExtracted
    ) -> events.NewFacts | None:
        header, enc_payload, signature = event.token.split(".")
        # add max of padding before decoding in case padding is missing (
        # extra padding will be ignored by Python's b64decode function anyways
        payload_data = base64.b64decode(enc_payload + "==").decode("utf-8")

        payload = json.loads(payload_data)
        k8s_info = payload["kubernetes.io"]
        pod_info = k8s_info["pod"]
        sa_info = k8s_info["serviceaccount"]

        token = ServiceAccountToken(
            subject=payload["sub"],
            issuer=payload["iss"],
            audience=payload["aud"],
            expires_at=payload["exp"],
            issued_at=payload["iat"],
            not_valid_before=payload["nbf"],
            namespace=k8s_info["namespace"],
            pod_name=k8s_info["pod"]["name"],
            pod_uid=k8s_info["pod"]["uid"],
            serviceaccount_name=k8s_info["serviceaccount"]["name"],
            serviceaccount_uid=k8s_info["serviceaccount"]["uid"],
            warn_after=k8s_info["warnafter"],
            raw=event.token,
        )
        ns = Namespace(name=k8s_info["namespace"])
        sa = ServiceAccount(name=sa_info["name"], ns=ns.name, token=token, expires_at=payload["exp"])
        # TODO check: if SA tokens can target other pods then the system where it was mounted on?
        pod = Pod(id=event.source_system_id, name=pod_info["name"], ns=ns.name)
        pod.service_account = sa

        sa_usage = Relation(name="uses", source=pod.id, destination=sa.id)
        # TODO: add token to loot (with ref to the system)
        # - extract the namespace, SA name and pod name (if necessary?)
        # - update topology and add parent node being the namespace (if not yet set)
        # - set `kind` of the system
        # - send updated topology to the UI
        #   - add the SA token as a small entity
        #   - everything is in NS compound node

        return events.NewFacts(
            entities=[
                ns,
                sa,
                pod,
            ],
            assets=[token],
            relations=[sa_usage],
        )

    async def analyze_extracted_environment_variables(
        self, event: events.EnvironmentVariablesExtracted
    ) -> Event | None:
        """Extract interesting facts from the environment variables.
        Kubernetes provides useful information as environment variables by default, such as:
        - the name of the pod
        - Kube-API endpoint
        - A list of all services that were running when a Container was created is available to that Container as environment variables.
            - if the `enableServiceLinks` flag is set
            - this is limited to services within the same namespace as the new Container's Pod and Kubernetes control plane services
        See [docs: Container Environmeent](https://kubernetes.io/docs/concepts/containers/container-environment/) for more.

        :param event: EnvironmentVariablesReceived  event with the source system and the variables
        :return: a new event with new facts, if there were any
        """
        pod_name = event.variables.get("HOSTNAME", "?")
        ns = Namespace(name="?")
        # TODO: env vars don't imply it's in a Pod?
        pod = Pod(id=event.source_system_id, name=pod_name, ns=ns)

        services = get_services_from_env_vars(event.variables)

        entities = [ns, pod]
        relations = []
        for svc, data in services.items():
            if svc == "KUBERNETES":
                entities.append(Namespace(name="kube-system"))
                sys = ApiServer(name=svc, ip=data["host"], ports=data["ports"])
            else:
                # services are either from same namespace as the pod, or 'kube-system'; assume same namespace as pod for now
                # TODO check if there are other services from the kube-system NS, which are added as env_var
                sys = Service(name=svc, ip=data["host"], ports=data["ports"], ns=ns)
            entities.append(sys)
            relations.append(
                Relation(source=pod.id, destination=sys.id, name="references", data="extracted from env_vars")
            )
        # TODO: analyze if URL is K8s DNS specific
        return events.NewFacts(entities=entities, relations=relations, assets=[])

    async def analyze_token_permissions(self, event: events.TokenCapabilitiesQueried) -> Event | None:
        sa = event.service_account
        for res_rule in event.resource_rules:
            sa.can += [f"{v} {res}" for v in res_rule["verbs"] for res in res_rule["resources"]]
        return events.NewFacts(entities=[sa])

    async def analyze_system_binaries(self, event: events.SystemBinariesListed) -> Event | None:
        # TODO add conditions to system: has_binary: <name>
        pass

    async def get_owning_system(self, node_id: str, relation_type: str | None = None) -> Entity | None:
        e = self.entities.get(node_id, None)
        # a system has no owner (for now)
        if isinstance(e, domain.System):
            return e

        for relation in self.relations:
            if relation.destination == node_id:
                if relation_type is None or relation.name == relation_type:
                    return self.entities[relation.source]
        print(f"No owning system of entity '{node_id}' found")
        return None


def get_services_from_env_vars(variables: dict[str, str]) -> dict:
    """Extract services from the environment variables.
    To extract services automatically populated by Kubernetes, a simple heuristic is used.
        1) look for all entries ending with `<xyz>_SERVICE_HOST`, the leading `<xyz>` is the service name
        2) for all service names get all other environment variables starting with this name
        3) get the host by reading the `<xyz>_SERVICE_HOST` value
        4) get all named ports by reading `<xyz>_SERVICE_PORT_<portname>`
            - if no named port was found, read `<xyz>_SERVICE_PORT` directly, which is the port number

    Group the dict of variables to a single entry with key kUBERNETES and value {host="10.96.0.1" and ports={"HTTPS": 443}}
    ```
    {
      'KUBERNETES_PORT': 'tcp://10.96.0.1:443',
      'KUBERNETES_PORT_443_TCP': 'tcp://10.96.0.1:443',
      'KUBERNETES_PORT_443_TCP_ADDR': '10.96.0.1',
      'KUBERNETES_PORT_443_TCP_PORT': '443',
      'KUBERNETES_PORT_443_TCP_PROTO': 'tcp',
      'KUBERNETES_SERVICE_HOST': '10.96.0.1',
      'KUBERNETES_SERVICE_PORT': '443',
      'KUBERNETES_SERVICE_PORT_HTTPS': '443',
    }```

    :param variables: a dict of environment variables and their values
    :return: a dict of services, with the service name as the key, and a dict with `host` and `ports` as its value
    """
    SVC_HOST_SFX = "_SERVICE_HOST"
    service_names = [s.replace(SVC_HOST_SFX, "") for s in variables.keys() if s.endswith(SVC_HOST_SFX)]

    svc_groups = defaultdict(dict)

    for svc in service_names:
        svc_vars = {k.replace(f"{svc}_", ""): v for k, v in variables.items() if k.startswith(svc)}
        host = svc_vars["SERVICE_HOST"]
        # specifically filter for named ports, which end with `SERVICE_PORT_<NAME>`
        ports = {k.split("_")[-1]: int(p) for k, p, in svc_vars.items() if "SERVICE_PORT_" in k}
        # if no named port is present, add the default port
        if len(ports) == 0:
            p = svc_vars["SERVICE_PORT"]  # this var should always be present
            ports[p] = int(p)
        svc_groups[svc] = {"host": host, "ports": ports}

    return svc_groups


async def execute_interactively():
    pass


@lru_cache
def get_campaign():
    return Campaign()


def merge_entities(e1: domain.Entity, e2: domain.Entity) -> domain.Entity:
    class1, class2 = type(e1), type(e2)
    # use the more specific class as the new class (as it contains more information)
    # if it's not a sublcass, then use the previous class, to avoid loosing facts
    ctor = class1 if issubclass(class1, class2) else class2
    attrs = {a: getattr(e, a) for e in [e1, e2] for a in e.model_fields_set}
    attrs["id"] = e1.id # keep the initial ID

    return ctor(**attrs)
