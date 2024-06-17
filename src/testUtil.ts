import { spawn } from 'child_process';
import { RaftNode } from './api';

export interface RaftNodeProcesses {
    started: Promise<void>,
    exit: () => Promise<void>,
    api: RaftNode,
}

export function startRaftNode(index: number): RaftNodeProcesses {
    const port = 8000 + index;
    const child = spawn(`ID=${index} PORT=${port} RPC_PORT=${50000 + index} yarn start-raft-server`, [], { shell: true });
    child.stderr.on('data', (data) => {
        console.error(`stderr[${index}]: ${data}`);
    });
    return {
        started: new Promise<void>(resolve => {
            child.stdout.on('data', (data) => {
                console.log(`stdout[${index}]: ${data}`);
                if (data.toString().includes('Web server started')) {
                    resolve();
                }
            });
        }),
        exit: () => new Promise(resolve => {
            console.log(`exiting server ${index}...`);
            child.kill('SIGTERM');
            child.on('close', (code) => {
                console.log(`exit[${index}]: ${code}`);
                resolve();
            });
        }),
        api: new RaftNode('localhost', port),
    };
}