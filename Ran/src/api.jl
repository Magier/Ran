module Api

using Oxygen
using HTTP

@get "/sessions" function(req::HTTP.Request)
    return "getting sessions not implemented"
end

@get "/greet" function(req::HTTP.Request)
    return "hello world!"
end

end