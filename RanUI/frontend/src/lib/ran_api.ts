import { campaign, api, type domain } from '$lib/domain/models';

type PendingRequest = {
    resolve: (value: any) => void;
    reject: (reason: any) => void;
};

export class RanAPI {
    socket!: WebSocket;
    private pendingRequests = new Map<string, PendingRequest>();
    private messageHandlers = new Map<string, (data: any) => void>();

    connect(url: string = "ws://localhost:8080/ws"): Promise<void> {
        console.info("Connecting to WebSocket at", url);
        console.log("websocket this", this)
        console.log("websocket this.socket", this.socket)
        return new Promise((resolve, reject) => {
            this.socket = new WebSocket(url);
            this.socket.onopen = () => {
                console.log("WebSocket connection established");
                resolve();
            };
            this.socket.onerror = (err) => {
                console.error("WebSocket error:", err);
                reject(err);
            };
            this.socket.onmessage = (event) => {
                this.handleMessage(event);
            };
            this.socket.onclose = () => {
                console.warn("❌ WebSocket connection closed");
                // Reject all pending requests on close
                this.pendingRequests.forEach((req) => {
                    req.reject(new Error("WebSocket closed"));
                });
                this.pendingRequests.clear();
            };
        });
    }

    private handleMessage(event: MessageEvent) {
        try {
            const message = JSON.parse(event.data);
            const { type, data, error } = message;

            // Check if this is a response to a pending request
            const pending = this.pendingRequests.get(type);
            if (pending) {
                this.pendingRequests.delete(type);
                if (error) {
                    pending.reject(new Error(error));
                } else {
                    pending.resolve(data);
                }
                return;
            }

            // Otherwise, call registered handler for push events
            const handler = this.messageHandlers.get(type);
            if (handler) {
                handler(data);
            } else {
                console.warn("Unhandled message type:", type, data);
            }
        } catch (err) {
            console.error("Failed to parse WebSocket message:", err);
        }
    }

    sendMessage<T = any>(type: string, params?: any): Promise<T> {
        return new Promise((resolve, reject) => {
            if (!this.socket || this.socket.readyState !== WebSocket.OPEN) {
                reject(new Error("WebSocket not connected"));
                return;
            }

            // Store pending request
            this.pendingRequests.set(type, { resolve, reject });

            const message = { type, params };
            this.socket.send(JSON.stringify(message));
        });
    }

    // Subscribe to push events (events not triggered by a request)
    on(type: string, handler: (data: any) => void) {
        this.messageHandlers.set(type, handler);
    }

    off(type: string) {
        this.messageHandlers.delete(type);
    }

    GetGraph(): Promise<api.Graph> {
        return this.sendMessage<api.Graph>("get-graph");
    }
    
    GetCampaignState(): Promise<api.CampaignState> {
        return this.sendMessage<api.CampaignState>("get-campaign-state");
    }
    GetArmory(): Promise<Array<domain.TTP>> {
        return this.sendMessage<Array<domain.TTP>>("get-armory");
    }   
    GetRunningPods(namespace: string): Promise<Array<api.K8sResource>> {
        return this.sendMessage<Array<api.K8sResource>>("get-running-pods", { namespace });
    }   

    ResetCampaign(): Promise<void> {
        return this.sendMessage<void>("reset-campaign");
    }

    GetApplicableTTPs(targetId: string): Promise<Array<domain.TTP>> {
        return this.sendMessage<Array<domain.TTP>>("get-applicable-ttps", { targetId });
    }
}


// export const ranSocket = new RanSocket(`ws://${window.location.host}/ws`);  

// let instance: RanSocket | null = null;

// export function getRanSocket(): RanSocket {
//     if (!instance) {
//         instance = new RanSocket(`ws://${window.location.host}/ws`);
//     }
//     return instance;
// }

// Singleton instance
const ranAPI = new RanAPI();

export function getRanAPI(): RanAPI {
    return ranAPI;
}

// Export the singleton
export { ranAPI };

// Export bound functions for convenience
export const connect = ranAPI.connect.bind(ranAPI);
export const on = ranAPI.on.bind(ranAPI);
export const off = ranAPI.off.bind(ranAPI);
export const GetGraph = ranAPI.GetGraph.bind(ranAPI);
export const GetCampaignState = ranAPI.GetCampaignState.bind(ranAPI);
export const GetArmory = ranAPI.GetArmory.bind(ranAPI);
export const GetRunningPods = ranAPI.GetRunningPods.bind(ranAPI);
export const ResetCampaign = ranAPI.ResetCampaign.bind(ranAPI);
export const GetApplicableTTPs = ranAPI.GetApplicableTTPs.bind(ranAPI);