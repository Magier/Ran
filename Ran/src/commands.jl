
abstract type Command <: Message end 

struct SendToUi <: Command
    type:: AbstractString
    data:: Any
end

# TODO: maybe support different types (e.g. mTLS, HTTP, DNS, etc.)
@Base.kwdef struct StartListener <: Command
    port:: Int = 1337
end