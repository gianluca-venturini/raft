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