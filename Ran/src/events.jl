
abstract type Event <: Message end

abstract type UiEvent <: Event end
struct ClientConnected <: UiEvent 
    id::String
    name:: String
end

struct ClientDisconnected <: UiEvent 
    id::String
end

