abstract type Message end

abstract type Command <: Message end 

struct SendToUi <: Command
    type:: AbstractString
    data:: Any
end

abstract type Event <: Message end

abstract type UiEvent <: Event end
struct ClientConnected <: UiEvent 
    id::String
    name:: String
end

struct ClientDisconnected <: UiEvent 
    id::String
end



abstract type Entity end

struct System <: Entity 
    id:: String
    name::String
end