abstract type Entity end

abstract type AbstractSystem <: Entity end

SystemId = AbstractString

Base.@kwdef struct System <: AbstractSystem
    id:: SystemId = string(uuid4())
    name::AbstractString
    os::Union{AbstractString,Nothing} = nothing
    accessLevel :: Union{AccessLevel,Nothing} = nothing
end

Base.@kwdef struct Listener <: AbstractSystem
    id:: AbstractString = string(uuid4())
    kind :: String = "Listener"
    name::AbstractString
    port :: Int
    protocol :: Union{AbstractString, Nothing} = nothing
    # os::Union{AbstractString,Nothing} = nothing
    # accessLevel :: Union{AccessLevel,Nothing} = nothing
end

Base.@kwdef struct Namespace <: Entity
    id :: AbstractString = string(uuid4())
    kind:: String = "Namespace"
    name:: AbstractString
end

abstract type AbstractToken <: Asset end


# JWT RFC: https://datatracker.ietf.org/doc/html/rfc7519#section-4
Base.@kwdef struct JWTToken <: AbstractToken
    subject:: Union{AbstractString, Nothing} = nothing
    audience:: Vector{AbstractString} = []
    issuer:: Union{AbstractString, Nothing} = nothing
    expiresAt:: Union{Int, Nothing} = nothing
    issuedAt:: Union{Int, Nothing} = nothing
    notValidBefore:: Union{Int, Nothing} = nothing
    raw:: Union{AbstractString, Nothing} = nothing
end


Base.@kwdef struct ServiceAccountToken <: AbstractToken
    jwtToken:: JWTToken # ServiceAccountToken is just a JWT token 
    # TODO verify if issuer is indicater of K8s api server?
    namespace:: Union{AbstractString, Nothing} = nothing
    podName:: Union{AbstractString, Nothing} = nothing
    podUid:: Union{AbstractString, Nothing} = nothing
    serviceAccountName:: Union{AbstractString, Nothing} = nothing
    serviceAccountUid:: Union{AbstractString, Nothing} = nothing
    warnAfter:: Union{Int, Nothing} = nothing
end


Base.@kwdef struct ServiceAccount <: Entity
    id :: AbstractString = string(uuid4())
    kind:: String = "ServiceAccount"
    ns:: Union{AbstractString, Namespace, Nothing} = nothing
    name:: AbstractString
    token:: Union{AbstractString, ServiceAccountToken, Nothing} = nothing
    expiresAt:: Union{Int, Nothing} = nothing
    can:: Vector{AbstractString} = []
end

Base.@kwdef struct Pod <: AbstractSystem
    id :: AbstractString = string(uuid4())
    kind:: String = "Pod"
    name:: AbstractString
    ns:: Union{AbstractString, Namespace, Nothing} = nothing
    serviceAccount:: Union{ServiceAccount, Nothing} = nothing
end


Base.@kwdef struct ApiServer <: AbstractSystem
    id :: AbstractString = string(uuid4())
    kind:: String = "KubeApiServer"
    name:: String = "API Server"
    ns:: Union{AbstractString, Namespace} = "kube-system"
    ip:: Union{AbstractString, Nothing} = nothing
    ports:: Dict{AbstractString, Union{Int, AbstractString}} = Dict()
end

Base.@kwdef struct Service <: Entity
    id :: AbstractString = string(uuid4())
    kind:: String = "Service"
    name:: AbstractString
    ns:: Union{AbstractString, Namespace, Nothing} = nothing
    ip:: Union{AbstractString, Nothing} = nothing
    ports:: Dict{AbstractString, Union{Int, AbstractString}} = Dict()
end