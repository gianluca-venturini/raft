import { spawn } from 'child_process';
import { RaftNode } from './api';

export interface RaftNodeProcesses {
    id: number,
    started: Promise<void>,
    leader: Promise<void>,
    exit: () => Promise<void>,
    api: RaftNode,
    storagePath: string,
}

export function startRaftNode(execId: string, index: number, numNodes: number, noElection: boolean = false): RaftNodeProcesses {
    const storagePath = `/tmp/node-${execId}-${index}`;
    const port = 8000 + index;
    const child = spawn('yarn', ['start-raft-server'], {
        env: {
            NUM_NODES: `${numNodes}`,
            ID: `${index}`,
            PORT: `${port}`,
            RPC_PORT: `${50000 + index}`,
            STORAGE_PATH: storagePath,
            NO_ELECTION: noElection ? 'true' : undefined,
        },
        shell: '/bin/bash',
    });
    child.stderr.on('data', (data) => {
        console.error(`stderr[${index}]: ${data}`);
    });
    let isLeaderResolve: () => void;
    const isLeaderPromise = new Promise<void>(resolve => {
        isLeaderResolve = resolve;
    });

    return {
        id: index,
        started: new Promise<void>(resolve => {
            let webServerStarted = false;
            let rpcServerStarted = false;
            child.stdout.on('data', (data) => {
                console.log(`stdout[${index}]: ${data}`);
                if (data.toString().includes('Web server started')) {
                    webServerStarted = true;
                }
                if (data.toString().includes('RPC server started')) {
                    rpcServerStarted = true;
                }
                if (data.toString().includes('Elected leader with majority votes')) {
                    isLeaderResolve();
                }
                if (webServerStarted && rpcServerStarted) {
                    resolve();
                }
            });
        }),
        leader: isLeaderPromise,
        exit: () => new Promise(resolve => {
            console.log(`exiting server ${index} [pid=${child.pid}]...`);
            child.kill('SIGTERM');
            child.on('close', (code) => {
                console.log(`exit[${index}]: ${code}`);
                resolve();
            });
        }),
        api: new RaftNode('localhost', port, `${index}`),
        storagePath,
    };
}