
import { fromPairs, times } from 'lodash';
import { v4 as uuidv4 } from 'uuid';

import { RaftNodeProcesses, startRaftNode } from './testUtil';
import { NotFoundError, RaftClient } from './api';

describe('integration 3 nodes', () => {
    integrationTests(3);
});

describe('integration 11 nodes', () => {
    integrationTests(11);
});

describe('integration single node', () => {
    let raftNode: RaftNodeProcesses;
    let raftClient: RaftClient;
    let execId: string;

    beforeEach(async () => {
        execId = uuidv4();
        raftNode = startRaftNode(execId, 0, 1);
        raftClient = new RaftClient({ '0': raftNode.api });
        // Wait until it's started
        await raftNode.started;
    });

    afterEach(async () => {
        await raftNode.exit();
    });

    it('elects itself as leader', async () => {
        await doWithRetry(async () => {
            const state = await raftNode.api.getState();
            if (state.role !== 'Leader') {
                throw new RetryError();
            }
        });
    });

    it('write and read a variable', async () => {
        await raftClient.setVar('foo', 42);
        expect(await raftClient.getVar('foo')).toBe(42);
    });

    it('persists the log on disk', async () => {
        await raftClient.setVar('foo', 42);
        await raftNode.exit();
        // Use the same execId to ensure the node is restarted with the same storage path
        raftNode = startRaftNode(execId, 0, 1);
        await raftNode.started;
        // Need to wait for the node to become leader to ensure
        // the log is applied
        await getLeaderNode([raftNode]);
        await new Promise(resolve => setTimeout(resolve, 1000));

        const value = await doWithRetry(async () => {
            // We may need to retry few times because the node is still starting up
            // and may not have committed the persisted log entry yet
            try {
                return await raftClient.getVar('foo')
            } catch (error) {
                if (error instanceof NotFoundError) {
                    // retry
                    throw new RetryError();
                }
            }
        });
        expect(value).toBe(42);
    });
});

describe('initial state', () => {
    let raftNodes: RaftNodeProcesses[];
    let raftClient: RaftClient;
    let execId: string;

    beforeEach(async () => {
        const numNodes = 11;
        execId = uuidv4();
        // Don't execute election so we can observe the initial state
        raftNodes = times(numNodes).map(index => startRaftNode(execId, index, numNodes, true));
        raftClient = new RaftClient(fromPairs(raftNodes.map((node, index) => [`${index}`, node.api])));
        // Wait until all servers are started
        await Promise.all(raftNodes.map(server => server.started));
    });

    afterEach(async () => {
        await Promise.all(raftNodes.map(server => server.exit()));
    });

    it('all nodes are followers at the beginning', async () => {
        for (const node of raftNodes) {
            console.log('testing node');
            const state = await node.api.getState();
            console.log('state', state);
            expect(state.role).toBe('Follower');
        }
    });
});

function integrationTests(numNodes: number) {
    let raftNodes: RaftNodeProcesses[];
    let raftClient: RaftClient;
    let execId: string;

    beforeEach(async () => {
        execId = uuidv4();
        raftNodes = times(numNodes).map(index => startRaftNode(execId, index, numNodes));
        raftClient = new RaftClient(fromPairs(raftNodes.map((node, index) => [`${index}`, node.api])));
        // Wait until all servers are started
        await Promise.all(raftNodes.map(server => server.started));
    });

    afterEach(async () => {
        await Promise.all(raftNodes.map(server => server.exit()));
    });

    it('do nothing', async () => {
        // Tests if nodes spin up and down correctly in beforeEach() and afterEach()
        await new Promise(resolve => setTimeout(resolve, 200));
    });

    describe('leader election', () => {
        it('one node is elected leader', async () => {
            let numLeaders = 0;
            for (let attempts = 0; attempts < 100; attempts++) {
                for (const node of raftNodes) {
                    if ((await node.api.getState()).role === 'Leader') {
                        console.log(`leader found: ${node.id}`);
                        numLeaders++;
                    }
                }
                if (numLeaders > 0) {
                    break;
                }
                await new Promise(resolve => setTimeout(resolve, 20));
            }
            expect(numLeaders).toBe(1);
        });
    });

    describe('variables', () => {
        it('read variable not found', async () => {
            await expect(raftClient.getVar('foo')).rejects.toThrow(NotFoundError);
        });

        it('write and read variable', async () => {
            await raftClient.setVar('foo', 42);
            expect(await raftClient.getVar('foo')).toBe(42);
        });

        it('write and read variable after leader restarts', async () => {
            await raftClient.setVar('foo', 42);
            expect(await raftClient.getVar('foo')).toBe(42);
            const leaderNode = await getLeaderNode(raftNodes);
            if (leaderNode) {
                await leaderNode.exit();
                // Restart the node that we just terminated
                const newNode = startRaftNode(execId, leaderNode.id, numNodes);
                // Replace the leader with the new node
                raftNodes[leaderNode.id] = newNode;
            } else (
                fail('No leader found')
            )
            await doWithRetry(async () => {
                try {
                    const result = await raftClient.getVar('foo');
                    expect(result).toBe(42);
                } catch (error) {
                    console.log('error', error);
                    if (error instanceof NotFoundError) {
                        throw new RetryError();
                    }
                    throw error;
                }
            });
        }, 30_000);
    });

    describe('log replication', () => {
        it('the leader start with a noop action in the log', async () => {
            const leaderNode = await getLeaderNode(raftNodes);
            expect(leaderNode).toBeDefined();
            const leaderNodeState = await leaderNode?.api.getState();
            console.log(leaderNodeState);
            expect(leaderNodeState?.log[0]?.command.type).toBe('Noop');
        });

        it('the leader evantually propagates the initial noop to all nodes', async () => {
            // wait for leader election
            await getLeaderNode(raftNodes);
            await checkOnAllNodes(raftNodes, async node => {
                const state = await node.api.getState();
                return state.log[0]?.command.type === 'Noop';
            });
        });
    });
}

async function getLeaderNode(raftNodes: RaftNodeProcesses[]): Promise<RaftNodeProcesses | undefined> {
    return doWithRetry(async () => {
        for (const node of raftNodes) {
            if ((await node.api.getState()).role === 'Leader') {
                return node;
            }
        }
        throw new RetryError();
    });
}

async function checkOnAllNodes(raftNodes: RaftNodeProcesses[], check: (node: RaftNodeProcesses) => Promise<boolean>): Promise<boolean> {
    return doWithRetry(async () => {
        for (const node of raftNodes) {
            if (!(await check(node))) {
                throw new RetryError();
            }
        }
        return true;
    });
}

class RetryError extends Error { }

async function doWithRetry<T>(fn: () => Promise<T>, maxAttempts: number = 10, timeoutMs: number = 500): Promise<T> {
    for (let attempts = 0; attempts < maxAttempts; attempts++) {
        try {
            return await fn();
        } catch (error) {
            if (!(error instanceof RetryError)) {
                throw error;
            }
        }
        await new Promise(resolve => setTimeout(resolve, timeoutMs));
    }
    throw new Error('Max attempts reached');
}
