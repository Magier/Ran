

function onStartListener(ev::StartListener)
    @info "start listener on port $(ev.port)"

    @async begin
        HTTP.WebSocket.open("ws://0.0.0.0:$(ev.port)") do ws
            for msg in ws
                println("got msg via listener: $msg")
            end
        end
    end

    @warn "post listener"
end

function startC2(bus:: MessageBus)
    @info "Starting C2"
    register!(bus, StartListener, onStartListener)
end 