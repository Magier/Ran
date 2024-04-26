
struct Envelope
    topic::Type{<:Message}
    data::Any
end

mutable struct MessageBus
    channel::Channel{Any}
    handlers::Dict{Type{<:Message}, Vector{Function}}
end
function MessageBus()
    return MessageBus(Channel(), Dict{Type{<:Message}, Vector{Function}}())
end

function handleEvents(bus::MessageBus)
    while true
        msg = take!(bus.channel)
        println("💻 RX $msg\n")

        handlers = get(bus.handlers, typeof(msg), [])
        if length(handlers) == 0
            println("$msg has no handlers")
        end

        for handler in handlers
            res = handler(msg)
            if typeof(res) <: Message
                @async put!(bus.channel, res)
            else
                @async [put!(bus.channel, r) for r in res]
            end
        end
    end
end

function register!(bus::MessageBus, msg::Type{<:Message}, fn::Function) 
    msgHandlers = get!(bus.handlers, msg, Function[])
    push!(msgHandlers, fn)

    function unsub()
        deleteat!(msgHandlers, findall(f->f==fn, msgHandlers))
    end
    return unsub
end


function publish!(bus::MessageBus, msg::Type{<:Message}, data::Union{Any, Nothing}=nothing)
    put!(bus.channel, Envelope(msg, data))
end

function publish!(bus::MessageBus, msg::T) where {T<:Message}
    put!(bus.channel, msg)
end