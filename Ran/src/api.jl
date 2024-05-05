# module Api

using UUIDs
using Oxygen
using HTTP
using JSON3


dynamicfiles("./static", "static")


@get "/sessions" function(req::HTTP.Request)
    return "getting sessions not implemented"
end


"""
Provide a single interface for wrapping and sending messages via the websocket
    Return true if the message was sent successfully, false otherwise
"""
function send(ws::HTTP.WebSocket, msg_type::String, data::Union{Dict, String, Vector}) :: Bool
    try
        msg = Dict("type" => msg_type, "data" => data)
        HTTP.send(ws, JSON3.write(msg))
    catch e
        @error "Error while sending msg: $e"
        return false
    end
    return true
end

function handleSendMessages(ws::HTTP.WebSocket, channel::Channel)
    @debug " >>> Ready for messages to send... Thread $(Threads.threadid())"
    for ev in channel
        send(ws, ev.type, ev.data)
    end
    @debug " . ...... done checking events on the channel $channel (Thead $(Threads.threadid())"
end

function parseCommand(data::Any) :: Union{Command, Nothing}
    data = strip(data)

    if startswith(data, "listen")
        return StartListener()
    end
    return nothing
end

function handleSocket(ws::HTTP.WebSocket, bus::MessageBus, clientId::String, channel::Channel)
    publish!(bus, ClientConnected(clientId, "UI client"))

    try
        for msgStr in ws
            msg = JSON3.read(msgStr)
            eventType = get(msg, "msg_type", Nothing)
            data = get(msg, "data", Nothing)

            if eventType == "terminal" 
                cmd = parseCommand(data)
                @info "got cmd: $cmd"

                if isnothing(cmd)
                    send(ws, "terminal", Dict("status" => "false", "message" => "Invalid command"))
                else
                    publish!(bus, cmd)
                end
            elseif eventType == "quit"
                send(ws, "terminal", "bye ~~")
                close(channel) 
                break
            end
        end
    catch e
        @error "Error websocket: $e"
    end

    publish!(bus, ClientDisconnected(clientId))
end


@websocket "/ws" function(ws::HTTP.WebSocket)
    bus = get!(ws.request.context, :BUS, nothing)
    clientId = get!(ws.request.context, :CLIENT_ID, nothing)
    channel = get!(ws.request.context, :SEND_CHANNEL, nothing)

    if isnothing(channel)
        @error "SEND_CHANNEL was not set for websocket"
    elseif isnothing(channel)
        @error "CLIENT_ID not set for websocket"
    elseif isnothing(bus)
        @error "BUS was not set properly for request"
    else
        @sync begin
            errormonitor(@async handleSendMessages($ws, $channel))
            errormonitor(@async handleSocket($ws, $bus, $clientId, $channel))
            # bind(channel, taskTx) # bind lifetime of both tasks via the channel, if one closes, the other will too
            # errormonitor(taskRx)
            # errormonitor(taskTx)
        end
    end
    # TODO: lifetime of tasks is still not okay, the following message is never reached:
    println("post socket")
end

function handleUiEvent(channels:: Dict{String, Channel}, event::SendToUi)
    @info "sending $(event.type) event to $(length(channels)) UIs"
    for ch in values(channels)
        put!(ch, event)
    end
end

function startApi(bus::MessageBus)
    channels = Dict{String, Channel}()

    unsub = register!(bus, UiEvent, (ev) -> handleUiEvent(channels, ev))
    unsub = register!(bus, SendToUi, (ev) -> handleUiEvent(channels, ev))

    register!(bus, ClientDisconnected, (ev::ClientDisconnected) -> begin
        @info "UI Client disconnected: $(ev.id)"
        ch = pop!(channels, ev.id, nothing)
        if !isnothing(ch)
            # close(ch)  # TODO: ideally this works, but currently it leads to TaskFailedException - fix it and then enable it!
        end
    end)

    function middle(handler)
        return function (req:: HTTP.Request)
            req.context[:BUS] = bus # provide access to the messagebus to the individual requests
            # req.context[:REGISTER] = registerConnection
            ch = Channel(100)
            clientId = string(uuid4())
            channels[clientId] = ch

            req.context[:CLIENT_ID] = clientId
            req.context[:SEND_CHANNEL] = ch

            res = handler(req)
            println(" >>> middleware post handler")
            println(res)
            return res
        end
    end

    serveparallel(host="0.0.0.0", port=8080, async=true, middleware=[middle])
end

# end