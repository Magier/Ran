abstract type Message end

abstract type Entity end

abstract type AbstractRelation end
Base.@kwdef struct  Relation <: AbstractRelation
    name::AbstractString
    source::AbstractString
    destination::AbstractString
    data::Union{AbstractString, Nothing} = nothing
end

Base.@kwdef struct System <: Entity 
    id:: AbstractString
    name::AbstractString
    os::Union{AbstractString,Nothing} = nothing
end

# abstract type SessionType end
# struct SimpleSession <: SessionType end
# struct ImplantSession <: SessionType end
@enum SessionType begin
    SimpleSession
    ImplantSession
end


include("./commands.jl")
include("./events.jl")
