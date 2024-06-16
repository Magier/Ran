struct Envelope
    topic::Type{<:Message}
    data::Any
end

mutable struct MessageBus
    channel::Channel{Any}
    handlers::Dict{Type{<:Message},Vector{Function}}
end
function MessageBus()
    return MessageBus(Channel(100), Dict{Type{<:Message},Vector{Function}}())
end

function handleEvents(bus::MessageBus)
    while true
        msg = take!(bus.channel)
        @info "🚌 RX $(nameof(typeof(msg)))"

        handlers = get(bus.handlers, typeof(msg), [])
        if length(handlers) == 0
            @warn "$msg has no handlers"
        end

        for handler in handlers
            res = handler(msg)

            if isnothing(res)
                continue
            end
            if typeof(res) <: Message
                errormonitor(Threads.@spawn put!(bus.channel, res))
            elseif !isnothing(res)
                errormonitor(Threads.@spawn [put!(bus.channel, r) for r in res])
            end
        end
    end
end

function register!(bus::MessageBus, msg::Type{<:Message}, fn::Function)
    msgHandlers = get!(bus.handlers, msg, Function[])
    push!(msgHandlers, fn)

    function unsub()
        deleteat!(msgHandlers, findall(f -> f == fn, msgHandlers))
    end
    return unsub
end


function publish!(bus::MessageBus, msg::Type{<:Message}, data::Union{Any,Nothing}=nothing)
    @debug "publish type $msg"
    put!(bus.channel, Envelope(msg, data))
end

function publish!(bus::MessageBus, msg::T) where {T<:Message}
    @debug "publish instance $msg"
    put!(bus.channel, msg)
end