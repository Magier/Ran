module Ran

include("api.jl")
using .Api

function main() 
    # create message bus
    # create campaign
    # create c2 adapter
    # start the API server
    Api.serve()
end


end

Ran.main()