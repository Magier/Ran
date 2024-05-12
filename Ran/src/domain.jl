abstract type Message end

abstract type Command <: Message end 

struct SendToUi <: Command
    data:: Dict{AbstractString, Any}
end

abstract type Event <: Message end

abstract type UiEvent <: Event end
struct ClientConnected <: UiEvent 
end

struct ClientDisConnected <: UiEvent 
end

