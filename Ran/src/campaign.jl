
mutable struct Campaign
    entities::Vector{}
end



function onClientConnected(ev::ClientConnected)
    armory = getArmory()
    topology = Dict(
        "entities" => [],
        "relations" => []
    )

    return SendToUi("armory", armory), SendToUi("topology", topology)
end

function startCampaign(bus::MessageBus)
    campaign = Campaign([])

    register!(bus, ClientConnected, onClientConnected)
end