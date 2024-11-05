---
Title: ""
Techinque : "Container Administration Command
TTP: T1609
Tactics: ["Execution"]
Type: Channel
PreCondition: ["Can_Pod/Exec", ['$KUBEAPI', 'kubectl'] ]
Effect: ["rce into container"]
---


## Description

Attackers who have permissions, can run malicious commands in containers in the cluster using exec command (“kubectl exec”). In this method, attackers can use legitimate images, such as an OS image (e.g., Ubuntu) as a backdoor container, and run their malicious code remotely by using “kubectl exec”.


## Command

### Kubectl

`kubectl exec <TARGET> [-c <CONTAINER>] -- <CMD>`


### cUrl

`-v=8` shows these commands
GET `curl https://$KUBEAPI/api/v0/namspaces/$NS/pods/$POD`
GET `curl -H "Sec-Websocket-Protocol=v5.channel.k8s.io" -H "User-Agent=kubectl/v1.30.0 (darwin/arm64) kubernetes/7c48c2b" https://$KUBEAPI/api/v0/namspaces/$NS/pods/$POD/exec?command=$CMD&stderr=true&stdout=true`
POST `curl https://$KUBEAPI/api/v0/namspaces/$NS/pods/$POD/exec?command=$CMD&stderr=true&stdout=true`
