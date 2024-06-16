module Ran

include("domain.jl")
include("messageBus.jl")
include("armory.jl")
include("./commands.jl")

include("analyzers.jl")
include("campaign.jl")
include("api.jl")
include("c2.jl")


function main()
    # create c2 adapter
    msgBus = MessageBus()

    startApi(msgBus)
    startCampaign(msgBus)
    startC2(msgBus)

    handleEvents(msgBus)
end

function julia_main()::Cint
    main()
    return 0 # if things finished successfully
end

export main

end