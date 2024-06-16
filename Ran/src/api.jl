# module Api

using UUIDs
using Oxygen
using HTTP
using JSON3
using StructTypes


Base.@kwdef struct UiMessage
    msgType::AbstractString
    data::Any = nothing
end
StructTypes.StructType(::Type{UiMessage}) = StructTypes.Struct()
# StructTypes.names(::Type{UiMessage}) = ((:msgType, :msg_type)) # does not work :(


dynamicfiles("./static", "static")


@get "/sessions" function (req::HTTP.Request)
    return "getting sessions not implemented"
end


"""
Provide a single interface for wrapping and sending messages via the websocket
    Return true if the message was sent successfully, false otherwise
"""
function send(ws::HTTP.WebSocket, msgType::String, data::Union{Dict,String,Vector})::Bool
    try
        msg = Dict("type" => msgType, "data" => data)
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

function parseCommand(msg::Dict{String,Any})::Union{Command,Nothing}
    eventType = get(msg, "msg_type", nothing)

    if eventType == "execute_ttp"
        ttpId = get(msg, "ttp_id", nothing)
        params = get(msg, "params", Dict())
        technique = get(msg, "technique", nothing)
        targetId = get(msg, "target", nothing)
        action = get(msg, "action", nothing)
        return PrepareTTP(
            ttp=ttpId,
            technique=technique,
            target=targetId,
            action=action,
            params=params
        )
    elseif eventType == "terminal"
        data = strip(msg.gdata)

        if startswith(data, "listen")
            return StartListener()
        end
    elseif eventType == "reset_campaign"
        return ResetCampaign()
    end
    return nothing
end



function handleSocket(ws::HTTP.WebSocket, bus::MessageBus, clientId::String, channel::Channel)
    publish!(bus, ClientConnected(clientId, "UI client"))

    try
        for msgStr in ws
            @info "  >> 📩 message from UI: '$msgStr'"
            msg = JSON3.read(msgStr, Dict)
            # msg = JSON3.read(msgStr, UiMessage)
            cmd = parseCommand(msg)

            if isnothing(cmd)
                # send(ws, "terminal", Dict("status" => "false", "message" => "Invalid command"))
            else
                if typeof(cmd) <: Quit
                    close(channel)
                end

                println("publishing cmd on bus!")
                publish!(bus, cmd)
            end
        end
    catch e
        @error e
    end

    publish!(bus, ClientDisconnected(clientId))
end


@websocket "/ws" function (ws::HTTP.WebSocket)
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
            errormonitor(Threads.@spawn handleSendMessages($ws, $channel))
            errormonitor(Threads.@spawn handleSocket($ws, $bus, $clientId, $channel))
            # bind(channel, taskTx) # bind lifetime of both tasks via the channel, if one closes, the other will too
            # errormonitor(taskRx)
            # errormonitor(taskTx)
        end
    end
    println("post socket")
end

function handleUiEvent(channels::Dict{String,Channel}, event::SendToUi)
    for ch in values(channels)
        put!(ch, event)
    end
end

function startApi(bus::MessageBus)
    channels = Dict{String,Channel}()

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
        return function (req::HTTP.Request)
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

    serveparallel(host="0.0.0.0", port=8080, async=true, middleware=[middle], access_log=nothing)
end

# end