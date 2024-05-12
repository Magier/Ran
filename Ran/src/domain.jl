abstract type Message end


abstract type Asset end

@enum Tactic begin
    InitialAccess = 1
    Execution = 2
    Persistence = 3
    PrivilegeEscalation = 4
    DefenseEvasion = 5
    CredentialAccess = 6
    Discovery = 7
    LateralMovement = 8
    Impact = 9
end

@enum Technique begin
    FileAndDirectoryDiscovery = 1083
    ExploitPublicFacingApp = 1190
    GatherVictimHostInformation = 1592
    ContainerAndResourceDiscovery = 1613
    ExploitationForPrivilegeEscalation = 1068
    ContainerServiceAccount = 9016  # MS-TA9016

    SystemNetworkConfigurationDiscovery = 1016
    StealApplicationAccessToken = 1528
    PermissionGroupsDiscovery = 1069
    PermissionGroupsDiscovery_CloudGroups = 1069003 # T1069.003"

    DeployContainer = 1610
end



@enum AccessLevel begin
    Nil = 0
    UserRead = 1
    UserWrite = 2
    UserExecute = 3
    RooWriteRead = 4
    RooWriteWrite = 5
    RooWriteExecute = 6
end


include("./domain/entities.jl")


abstract type AbstractRelation end
Base.@kwdef struct  Relation <: AbstractRelation
    name::AbstractString
    source::AbstractString
    destination::AbstractString
    data::Union{AbstractString, Nothing} = nothing
end


# abstract type SessionType end
# struct SimpleSession <: SessionType end
# struct ImplantSession <: SessionType end
@enum SessionType begin
    SimpleSession
    ImplantSession
end


include("./commands.jl")
include("./events.jl")
