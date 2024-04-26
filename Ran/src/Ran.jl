module Ran

include("domain.jl")
include("messageBus.jl")
include("armory.jl")
include("campaign.jl")
include("api.jl")


function main() 
    # create c2 adapter
    msgBus = MessageBus()

    startApi(msgBus)
    startCampaign(msgBus)

    handleEvents(msgBus)
end

export main

end