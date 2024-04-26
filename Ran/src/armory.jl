@enum Tactic begin
    InitialAccess = 1
    Execution = 2
end

@enum Technique begin
    ExploitPublicFacingApp = 1190
end


Base.@kwdef struct ExploitParams 
    endpoint:: String
    params:: Dict
    method:: String
end

Base.@kwdef struct TTP 
    id:: String
    technique :: String
    name :: String
    action :: Union{String, Nothing} = nothing
    cmd_args :: Union{String, Nothing} = nothing
    tactics :: Vector{String}
    ms_id :: String = ""
    requires :: Union{Dict,Nothing} = Dict()
    params :: Union{ExploitParams, Nothing} = nothing
end

function loadArmory() :: Vector{TTP}

end