
abstract type Command <: Message end 

struct SendToUi <: Command
    type:: AbstractString
    data:: Any
end

@Base.kwdef struct StartListener <: Command
    port:: Int = 1337
end