using UUIDs
using Sockets

function handleNewImplant(conn)
    id = uuid4()
    println("New implant connected $id")

    while !eof(conn)
        msg = readline(conn)
        println(" >>> Request [$id]: $msg")
        write(conn, "ack $msg")
    end

    println("done with implant $id")
end

function onStartListener(ev::StartListener)
    @info "start listener on port $(ev.port)"

    # HTTP.serve!(handleNewImplant, "0.0.0.0",ev.port; async=true)
    errormonitor(@async begin
        server = Sockets.listen(ev.port)
        while true # TODO maybe create Event to stop  here and return the event as result event from this fn, so the event can be set somewhere else
            sock = Sockets.accept(server)
            @async handleNewImplant(sock)
        end
    end)
    @warn "post listener"
end

function startC2(bus:: MessageBus)
    println("starting C2")
    register!(bus, StartListener, onStartListener)
end 