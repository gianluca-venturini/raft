import fetch from 'node-fetch';

export class NotFoundError extends Error { }

interface RaftNodeState {
    role: 'Follower' | 'Candidate' | 'Leader';
}

export class RaftNode {
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
            throw new Error('Network response was not ok');
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
            throw new Error(`Network response was not ok ${response.status} ${response.statusText}`);
        }
        const data = await response.json() as TResponse;
        return data;
    }
}