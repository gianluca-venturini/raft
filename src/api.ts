import fetch from 'node-fetch';

export class NotFoundError extends Error {}

export async function getVar(key: string): Promise<number> {
    const response = await fetch(`http://localhost:8000/variable?key=${key}`);
    if (!response.ok) {
        if (response.status === 404) {
            throw new NotFoundError();
        }
        throw new Error('Network response was not ok');
    }
    const data = await response.json() as number;
    return data;
};

export async function setVar(key: string, value: number): Promise<void> {
    const response = await fetch(`http://localhost:8000/variable`, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json'
        },
        body: JSON.stringify({ key, value }),
    });
    if (!response.ok) {
        throw new Error(`Network response was not ok ${response.status} ${response.statusText}`);
    }
};

interface RaftNodeState {
    role: 'Follower' | 'Candidate' | 'Leader';
}

export async function getState(): Promise<RaftNodeState> {
    const response = await fetch(`http://localhost:8000/state`);
    if (!response.ok) {
        throw new Error('Network response was not ok');
    }
    const data = await response.json() as RaftNodeState;
    return data;
};