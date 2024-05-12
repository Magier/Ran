using UUIDs


abstract type Command <: Message end 

struct SendToUi <: Command
    type:: AbstractString
    data:: Any
end

# TODO: maybe support different types (e.g. mTLS, HTTP, DNS, etc.)
Base.@kwdef struct StartListener <: Command
    port:: Int = 1337
end

Base.@kwdef struct StopListener <: Command
    listenerId:: AbstractString
end

struct Quit <: Command end

Base.@kwdef struct PrepareTTP <: Command
    ttp:: AbstractString
    target:: Union{Entity, AbstractString, Nothing} = nothing  # depending on the TTP the target may be inferred
    technique:: Union{AbstractString, Nothing} = nothing
    params:: Union{Dict, Nothing} = nothing
    action:: Union{AbstractString, Nothing} = nothing
end

Base.@kwdef struct ExecuteActionOnTarget <: Command
    id :: AbstractString = string(uuid4())
    target :: AbstractString
    action:: AbstractString = "execute"
end