# module Api

using Oxygen
using HTTP
using JSON3


dynamicfiles("./static", "static")


@get "/sessions" function(req::HTTP.Request)
    return "getting sessions not implemented"
end


function handleSendMessages(ws::HTTP.WebSocket, channel)
    @warn " >>> Ready for messages to send... Thread $(Threads.threadid())"
    # while true
        # ev = take!(channel)
    for ev in channel
        HTTP.send(ws, JSON3.write(ev.data))
    end
end

function handleSocket(ws::HTTP.WebSocket, bus::MessageBus) 
    publish!(bus, ClientConnected())
    @info "handle socket $(Threads.threadid())"

    for msgStr in ws
        msg = JSON3.read(msgStr)
        eventType = get(msg, "event_type", Nothing)
        data = get(msg, "data", Nothing)

        if eventType == "terminal" 
            HTTP.send(ws, JSON3.write(Dict("type" => "terminal", "result" => data)))
        elseif eventType == "quit"
            HTTP.send(ws, JSON3.write(Dict("type"=> "terminal", "result" => "bye ~~")))
            break
        end
    end
    println("~~~ post socket")
end


@websocket "/ws" function(ws::HTTP.WebSocket)
    @info ">> websocket Handler: Thread $(Threads.threadid())"
    bus = get!(ws.request.context, :BUS, nothing)
    channel = get!(ws.request.context, :SEND_CHANNEL, nothing)

    if isnothing(channel)
        println("SEND channel was not set for websocket")
    elseif isnothing(bus)
        println("bus was not set properly for request")
    else
        @sync begin
            @async handleSendMessages(ws, channel)
            @async handleSocket(ws, bus)
        end
    end
    println("post socket")
end

function handleUiEvent(channels, event::Any)
    for ch in channels
        put!(ch, event)
    end
end

function startApi(bus::MessageBus)
    channels = []
    # unsubConnManager = register!(bus, ClientConnected, onNewClient)
    # unsubConnManager = register!(bus, ClientConnected, (ws) -> append!(connHandler, ws))

    unsub = register!(bus, UiEvent, (ev) -> handleUiEvent(channels, ev))
    unsub = register!(bus, SendToUi, (ev) -> handleUiEvent(channels, ev))

    function middle(handler)
        return function (req)
            req.context[:BUS] = bus # provide access to the messagebus to the individual requests
            # req.context[:REGISTER] = registerConnection
            ch = Channel(100)
            req.context[:SEND_CHANNEL] = ch

            push!(channels, ch)
            res = handler(req)
            return res
        end
    end

    serveparallel(host="0.0.0.0", port=8080, async=true, middleware=[middle])
end

# end