import { spawn } from 'child_process';

export interface RaftNodeProcesses {
    started: Promise<void>,
    exit: () => Promise<void>
}

export function startRaftNode(index: number): RaftNodeProcesses {
    const child = spawn(`PORT=${8000 + index} RPC_PORT=${50000 + index} yarn start-raft-server`, [], { shell: true });
    child.stderr.on('data', (data) => {
        console.error(`stderr[${index}]: ${data}`);
    });
    return {
        started: new Promise<void>((resolve, reject) => {
            child.stdout.on('data', (data) => {
                console.log(`stdout[${index}]: ${data}`);
                if (data.toString().includes('Web server started')) {
                    resolve();
                }
            });
        }),
        exit: () => new Promise((resolve, reject) => {
            console.log(`exiting server ${index}...`);
            child.kill('SIGTERM');
            child.on('close', (code) => {
                console.log(`exit[${index}]: ${code}`);
                resolve();
            });
        })
    };
}