 function analyzeEnvironmentVariables( event:: EnvironmentVariablesExtracted) :: Union{Event,Nothing}
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
    podName = get(event.variables, "HOSTNAME", "?")
    ns = Namespace(name="?")
    # TODO: env vars don't imply it's in a Pod?
    pod = Pod(id=event.sourceSystemId, name=podName, ns=ns)

    services = getServicesFromEnvVars(event.variables)

    entities = [ns, pod]
    relations = []
    for (svc, data) in (services)
        if svc == "KUBERNETES"
            push!(entities, Namespace(name="kube-system"))
            sys = ApiServer(name=svc, ip=data["host"], ports=data["ports"])
        else
            # services are either from same namespace as the pod, or 'kube-system'; assume same namespace as pod for now
            # TODO check if there are other services from the kube-system NS, which are added as env_var
            sys = Service(name=svc, ip=data["host"], ports=data["ports"], ns=ns)
        end

        push!(entities, sys)
        push!(relations, Relation(source=pod.id, destination=sys.id, name="references", data="extracted from env_vars"))
    end
    # TODO: analyze if URL is K8s DNS specific
    return NewFacts(entities=entities, relations=relations, assets=[])
end



function getServicesFromEnvVars(variables:: Dict{String, String}) :: Dict
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
    serviceNames = [replace(s, SVC_HOST_SFX => "") for s in keys(variables) if endswith(s, SVC_HOST_SFX)]

    svcGroups = Dict()

    for svc in serviceNames
        svcVars = Dict(replace(k, "$(svc)_" => "") => v for (k, v) in variables if startswith(k, svc))
        host = svcVars["SERVICE_HOST"]
        # specifically filter for named ports, which end with `SERVICE_PORT_<NAME>`
        ports = Dict(split(k, "_")[end] => parse(Int, p) for (k, p) in svcVars if occursin("SERVICE_PORT_", k))
        # if no named port is present, add the default port
        if length(ports) == 0
            p = svcVars["SERVICE_PORT"]  # this var should always be present
            ports[p] = parse(Int, p)
        end
        svcGroups[svc] = Dict("host"=> host, "ports"=> ports)
    end

    return svcGroups
end