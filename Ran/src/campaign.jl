
mutable struct Campaign
    entities::Vector{Entity}
    relations::Vector{AbstractRelation}
end


function structToDict(s::Any) :: Dict{AbstractString, Any}
    return Dict(string(key)=>getfield(s, key) for key ∈ fieldnames(typeof(s)))
end


function onClientConnected(ev::ClientConnected, campaign:: Campaign)
    armory = getArmory()
    topology = structToDict(campaign)
    return SendToUi("armory", armory), SendToUi("topology", topology)
end


function onListenerReady(ev::ListenerReady, campaign:: Campaign)
    @info("  [🧵$(Threads.threadid())][Campaign] listener ready")

    push!(campaign.entities, System(id=ev.id, name="Listener $(ev.port)"))

    topology = structToDict(campaign)
    return SendToUi("topology", topology)
end


function onSessionStarted(ev::SessionStarted, campaign:: Campaign)
    @debug("  [🧵$(Threads.threadid())][Campaign] session started")

    println("session event: $ev")
    push!(campaign.entities, System(id=ev.id, name=ev.hostname, os=ev.os, accessLevel=UserExecute ))
    # add direction of command (listener commands the target system)
    push!(campaign.relations, Relation(name="simple listener", source=ev.listenerId, destination=ev.id))

    topology = structToDict(campaign)
    return SendToUi("topology", topology)
end

function onSessionEnded(ev::SessionEnded, campaign:: Campaign)
    @debug("  [🧵$(Threads.threadid())][Campaign] session ended")
    # TODO remove system from campaign
    return []
end


function startCampaign(bus::MessageBus)
    campaign = Campaign([], [])
    register!(bus, ClientConnected, (ev) -> onClientConnected(ev, campaign))
    register!(bus, ListenerReady, (ev) -> onListenerReady(ev, campaign))
    register!(bus, SessionStarted, (ev) -> onSessionStarted(ev, campaign))
    register!(bus, SessionEnded, (ev) -> onSessionEnded(ev, campaign))
end