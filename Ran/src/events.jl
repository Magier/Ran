
abstract type Event <: Message end

abstract type UiEvent <: Event end
struct ClientConnected <: UiEvent 
    id::String
    name:: String
end

struct ClientDisconnected <: UiEvent 
    id::String
end


struct ListenerReady <: Event 
    id::AbstractString
    port::Int
    protocol::AbstractString
    task::Task
end

Base.@kwdef struct SessionStarted <: Event
    id::AbstractString
    listenerId::AbstractString
    type::SessionType = SimpleSession
    hostname::AbstractString = ""
    user::AbstractString = ""
    os:: AbstractString = ""
end

struct SessionEnded <: Event
end

Base.@kwdef struct ActionExecuted <: Event
    sessionId::AbstractString
    actionId::AbstractString
    target::Union{AbstractString, Nothing} = nothing
    # action::TTP
    output::Union{AbstractString, Nothing} = nothing
end

struct EnvironmentVariablesExtracted <: Event
    sourceSystemId::AbstractString
    variables::Dict{String, String}
end

struct ServiceAccountTokenExtracted <: Event
    rawToken::AbstractString
    sourceSystemId::SystemId
end

Base.@kwdef struct NewFacts <: Event
    # data:: Vector{Union{Entity, Relation, Asset}} = []
    entities:: Vector{Entity} = []
    relations:: Vector{Relation} = []
    assets:: Vector{Asset} = []
end