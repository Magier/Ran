import createClient from 'openapi-fetch';
import type { paths } from '$lib/api/gen_types';
import type {
	Graph,
	CampaignState,
	TTP,
	AttackFlow,
	ExecuteActionCmd,
	K8sResource
} from '$lib/api';

type PendingRequest = {
    resolve: (value: any) => void;
    reject: (reason: any) => void;
};

export class RanAPI {
    socket!: WebSocket;
    private pendingRequests = new Map<string, PendingRequest>();
    private messageHandlers = new Map<string, (data: any) => void>();
    private restClient = createClient<paths>({ baseUrl: '' }); // Use relative URLs

    connect(url?: string): Promise<void> {
        // Construct WebSocket URL from current location if not provided
        if (!url) {
            const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
            url = `${protocol}//${window.location.host}/ws`;
        }
        console.info("Connecting to WebSocket at", url);
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
        let msgType: string;
        let data: any;
        let error: any;
        try {
            ({ type: msgType, data, error } = JSON.parse(event.data));
        } catch (err) {
            console.error("Failed to parse WebSocket message:", err, event.data);
            return;
        }
        
        try {
            // Check if this is a response to a pending request
            const pending = this.pendingRequests.get(msgType);
            if (pending) {
                this.pendingRequests.delete(msgType);
                if (error) {
                    pending.reject(error);
                } else if (data === undefined) {
                    pending.reject("No data in response");
                } else {
                    pending.resolve(data);
                }
                return;
            }

            // Otherwise, call registered handler for push events
            const handler = this.messageHandlers.get(msgType);
            if (handler) {
                try {
                    handler(data);
                } catch (err) {
                    console.error("Error in message handler for type:", msgType, err);
                }
            } else {
                console.debug("Unhandled message type:", msgType, data);
            }
        } catch (err) {
            console.error("Failed to handle WebSocket message:", err, event.data);
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
            console.info("Sending WebSocket message:", message);
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

    // REST API Methods (auto-generated from OpenAPI spec)
    async GetGraph(): Promise<Graph> {
        const { data, error } = await this.restClient.GET('/api/graph');
        if (error) throw new Error('Failed to get graph');
        return data;
    }
    
    async GetCampaignState(): Promise<CampaignState> {
        const { data, error } = await this.restClient.GET('/api/campaign-state');
        if (error) throw new Error('Failed to get campaign state');
        return data;
    }

    async GetArmory(): Promise<Array<TTP>> {
        const { data, error } = await this.restClient.GET('/api/armory');
        if (error) throw new Error('Failed to get armory');
        return data;
    }

    async GetApplicableTTPs(targetId: string): Promise<Array<TTP>> {
        if (!targetId) {
            console.warn("GetApplicableTTPs called with empty targetId");
            return [];
        }

        const { data, error } = await this.restClient.GET('/api/applicable-ttps', {
            params: { query: { targetId } }
        });
        console.log("Applicable TTPs data:", data, "error:", error, "for targetId:", targetId);
        if (error) {
            console.warn('Failed to get applicable TTPs', error);
            return [];
        }
        return data;
    }

    async GetFlow(): Promise<AttackFlow> {
        const { data, error } = await this.restClient.GET('/api/flow');
        if (error) throw new Error('Failed to get flow');
        return data;
    }

    async ExportAttackFlow(): Promise<any> {
        const { data, error } = await this.restClient.GET('/api/flow/export');
        if (error) throw new Error('Failed to export attack flow');
        return data;
    }

    async SaveFlow(path: string) {
        const { data, error } = await this.restClient.POST('/api/flow/save', {
            body: { path }
        });
        if (error) throw new Error('Failed to save flow');
        return data;
    }

    async ExecuteAction(cmd: ExecuteActionCmd) {
        const { data, error } = await this.restClient.POST('/api/action/execute', {
            body: cmd
        });
        if (error) throw new Error('Failed to execute action');
        return data;
    }

    async ResetCampaign() {
        const { data, error } = await this.restClient.POST('/api/campaign/reset');
        if (error) throw new Error('Failed to reset campaign');
        return data;
    }

    async GetRunningPods(namespace?: string): Promise<Array<K8sResource>> {
        const { data, error } = await this.restClient.GET('/api/pods/running', {
            params: { query: { namespace } }
        });
        if (error) throw new Error(error.error);
        return data;
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
export const GetApplicableTTPs = ranAPI.GetApplicableTTPs.bind(ranAPI);
export const GetFlow = ranAPI.GetFlow.bind(ranAPI);
export const ExportAttackFlow = ranAPI.ExportAttackFlow.bind(ranAPI);
export const SaveFlow = ranAPI.SaveFlow.bind(ranAPI);
export const ExecuteAction = ranAPI.ExecuteAction.bind(ranAPI);
export const ResetCampaign = ranAPI.ResetCampaign.bind(ranAPI);
export const GetRunningPods = ranAPI.GetRunningPods.bind(ranAPI);