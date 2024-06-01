
mutable struct Campaign
    entities::Vector{Entity}
    relations::Vector{AbstractRelation}
    assets::Vector{Asset}
    armory::Dict{AbstractString, TTP}
end


function structToDict(s::Any) :: Dict{AbstractString, Any}
    return Dict(string(key)=>getfield(s, key) for key ∈ fieldnames(typeof(s)))
end


function onClientConnected(ev::ClientConnected, campaign:: Campaign)
    topology = structToDict(campaign)
    return SendToUi("armory", campaign.armory), SendToUi("topology", topology)
end


function onListenerReady(ev::ListenerReady, campaign:: Campaign)
    push!(campaign.entities, Listener(id=ev.id, name="Listener $(ev.port)", port=ev.port, protocol="tcp"))

    topology = structToDict(campaign)
    return SendToUi("topology", topology)
end


function onSessionStarted(ev::SessionStarted, campaign:: Campaign)
    @debug("  [🧵$(Threads.threadid())][Campaign] session started")

    push!(campaign.entities, System(id=ev.id, name=ev.hostname, os=ev.os, accessLevel=UserExecute ))
    # add direction of command (listener commands the target system)
    push!(campaign.relations, Relation(name="simple shell", source=ev.listenerId, destination=ev.id))

    topology = structToDict(campaign)
    return SendToUi("topology", topology)
end

function onSessionEnded(ev::SessionEnded, campaign:: Campaign)
    @debug("  [🧵$(Threads.threadid())][Campaign] session ended")
    # TODO remove system from campaign
    return []
end


function onPrepareTTP(ev::PrepareTTP, campaign:: Campaign)
    ttp = get(campaign.armory, ev.ttp, nothing)
    return ExecuteActionOnTarget(ttp=ttp, target=ev.target)
end

function onActionExecuted(ev::ActionExecuted, campaign::Campaign)
    ttp = get(campaign.armory, ev.actionId, nothing)
    if isnothing(ttp)
        @error("Could not find TTP with id $(ev.actionId)")
        return
    end

    if !isnothing(ttp.postProcess)
        ev = ttp.postProcess(ev)
        if !isnothing(ev)
            return ev
        end
    end
end

function onNewFacts(ev::NewFacts, campaign::Campaign)
    # merge with existing entities
    append!(campaign.entities, ev.entities)
    append!(campaign.relations, ev.relations)
    append!(campaign.assets, ev.assets)

    topology = structToDict(campaign)
    return SendToUi("topology", topology)
end 

function resetCampaign(ev::ResetCampaign, campaign::Campaign)
    # keep only the running listeners
    filter!(e -> e isa Listener, campaign.entities)
    campaign.relations = []
    campaign.assets = []
    # TODO: send commands to kill running sessions

    topology = structToDict(campaign)
    return SendToUi("topology", topology)
end

function startCampaign(bus::MessageBus)
    ttps = getArmory()
    armory = Dict(ttp.id => ttp for ttp in ttps)

    campaign = Campaign([], [], [], armory)
    register!(bus, ClientConnected, (ev) -> onClientConnected(ev, campaign))
    register!(bus, ListenerReady, (ev) -> onListenerReady(ev, campaign))
    register!(bus, SessionStarted, (ev) -> onSessionStarted(ev, campaign))
    register!(bus, SessionEnded, (ev) -> onSessionEnded(ev, campaign))
    register!(bus, PrepareTTP, (ev) -> onPrepareTTP(ev, campaign))
    register!(bus, ResetCampaign, (ev) -> resetCampaign(ev, campaign))

    register!(bus, ActionExecuted, (ev) -> onActionExecuted(ev, campaign))
    register!(bus, NewFacts, (ev) -> onNewFacts(ev, campaign))

    # analyzers (may yield new facts)
    register!(bus, EnvironmentVariablesExtracted, analyzeEnvironmentVariables)
    register!(bus, ServiceAccountTokenExtracted, analyzeExtractedServiceAccountToken)
end