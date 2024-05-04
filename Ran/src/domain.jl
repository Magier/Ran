abstract type Message end

include("./commands.jl")
include("./events.jl")



abstract type Entity end

struct System <: Entity 
    id:: String
    name::String
end