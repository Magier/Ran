
mutable struct Campaign
    entities::Vector{}
end



function onClientConnected(ev::ClientConnected)
    # send armory
    # send current topology
    armory = getArmory()
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