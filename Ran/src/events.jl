
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

struct ActionExecuted <: Event
    sessionId::AbstractString
    action::ExecuteActionOnTarget
    output::AbstractString
end

struct EnvironmentVariablesExtracted <: Event
    sourceSystemId::AbstractString
    variables::Dict{String, String}
end