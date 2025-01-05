import { keys, values } from 'lodash';
import fetch from 'node-fetch';

export class NotFoundError extends Error { }
export class NotLeaderError extends Error {
    constructor(public leaderId: string | undefined) {
        super(`Not leader. The leader is possibly ${leaderId}.`);
    }
}

interface RaftNodeState {
    role: 'Follower' | 'Candidate' | 'Leader';
}

interface RaftClientApi {
    getVar(key: string): Promise<number>;
    setVar(key: string, value: number): Promise<void>;
}

/** Communicates directly with a single node.
 * If the node is not the leader, certain actions may fail. */
export class RaftNode implements RaftClientApi {
    constructor(private host: string, private port: number) { }

    async getVar(key: string): Promise<number> {
        console.log('getVar', key);
        return this.getRequest<number>('/variable', { key });
    };

    async setVar(key: string, value: number): Promise<void> {
        console.log('setVar', key, value);
        return this.postRequest<void>('/variable', { key, value });
    };

    async getState(): Promise<RaftNodeState> {
        console.log('getState');
        return this.getRequest<RaftNodeState>('/state', {});
    };

    private async getRequest<TResponse>(path: string, params: Record<string, string>): Promise<TResponse> {
        const url = new URL(`http://${this.host}:${this.port}${path}`);
        const urlSearchParams = new URLSearchParams(params);
        url.search = urlSearchParams.toString();
        const response = await fetch(url.toString());
        if (!response.ok) {
            if (response.status === 404) {
                throw new NotFoundError();
            }
            if (response.status === 308) {
                const { leaderId }: { leaderId: string | undefined } = await response.json();
                throw new NotLeaderError(leaderId);
            }
            throw new Error(`Network response was not ok ${response.status}`);
        }
        const data = await response.json() as TResponse;
        return data;
    }

    private async postRequest<TResponse>(path: string, params: Record<string, unknown>): Promise<TResponse> {
        const url = new URL(`http://${this.host}:${this.port}${path}`);
        const response = await fetch(url.toString(), {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json'
            },
            body: JSON.stringify(params),
        });
        if (!response.ok) {
            if (response.status === 404) {
                throw new NotFoundError();
            }
            throw new Error(`Network response was not ok ${response.status} ${response.statusText}`);
        }
        const data = await response.json() as TResponse;
        return data;
    }
}

export class RaftClient implements RaftClientApi {
    private maxAttempts = 10;

    constructor(private nodes: { [nodeId: string]: RaftNode }) { }

    async getVar(key: string): Promise<number> {
        return this.attemptApiCall(node => node.getVar(key));
    }

    async setVar(key: string, value: number): Promise<void> {
        return this.attemptApiCall(node => node.setVar(key, value));
    }

    private getRandomNode(): RaftNode {
        const randomNodeId = keys(this.nodes)[Math.floor(Math.random() * keys(this.nodes).length)];
        return this.nodes[randomNodeId];
    }

    /** Executes the api call on a random node. If not the leader the operation may fail
     * with a 503 status code and a leaderId in the response.
     */
    private async attemptApiCall<T>(fn: (node: RaftNode) => Promise<T>): Promise<T> {
        let node = this.getRandomNode();
        for (let attempt = 0; attempt < this.maxAttempts; attempt++) {
            try {
                return await fn(node);
            } catch (error) {
                if (error instanceof NotLeaderError) {
                    const leaderId = error.leaderId;
                    if (!leaderId) {
                        // No leader known, try again with a random node
                        node = this.getRandomNode();
                        // Wait some time before trying again because a leader may be elected soon
                        // This time should be more than it takes to elect a leader
                        await new Promise(resolve => setTimeout(resolve, 1000));
                        continue;
                    }
                    if (!this.nodes[leaderId]) {
                        throw new Error(`Leader ${leaderId} not known to client`);
                    }
                    node = this.nodes[leaderId];
                    continue;
                }
                throw error;
            }
        }
        throw new Error('Max attempts reached');
    }
}