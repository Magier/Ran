module Ran

include("domain.jl")
include("messageBus.jl")
include("armory.jl")
include("./commands.jl")

include("analyzers.jl")
include("campaign.jl")
include("api.jl")
include("c2.jl")


export main
function main(ARGS)
    # create c2 adapter
    msgBus = MessageBus()

    startApi(msgBus)
    startCampaign(msgBus)
    startC2(msgBus)

    handleEvents(msgBus)
end
@main

end