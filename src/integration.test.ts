
import { fromPairs, times } from 'lodash';

import { RaftNodeProcesses, startRaftNode } from './testUtil';
import { NotFoundError, RaftClient } from './api';

describe('integration 3 nodes', () => {
    integrationTests(3);
});

// Enable below to stress test the system

// describe('integration 11 nodes', () => {
//     integrationTests(11);
// });

// describe('integration 101 nodes', () => {
//     integrationTests(101);
// });

function integrationTests(numNodes: number) {
    let raftNodes: RaftNodeProcesses[];
    let raftClient: RaftClient;

    beforeEach(async () => {
        raftNodes = times(numNodes).map(index => startRaftNode(index, numNodes));
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
        it('all nodes are followers at the beginning', async () => {
            for (const node of raftNodes) {
                console.log('testing node');
                const state = await node.api.getState();
                console.log('state', state);
                expect(state.role).toBe('Follower');
            }
        });

        it('one node is elected leader', async () => {
            let numLeaders = 0;
            for (let attempts = 0; attempts < 10; attempts++) {
                for (const node of raftNodes) {
                    if ((await node.api.getState()).role === 'Leader') {
                        numLeaders++;
                    }
                }
                if (numLeaders > 0) {
                    break;
                }
                await new Promise(resolve => setTimeout(resolve, 500));
            }
            expect(numLeaders).toBe(1);
        });
    });

    describe('variables', () => {
        it('read variable not found', async () => {
            await expect(raftClient.getVar('foo')).rejects.toThrow(NotFoundError);
        });

        // it('write and read variable same server', async () => {
        //     await raftClient.setVar('foo', 42);
        //     expect(await raftClient.getVar('foo')).toBe(42);
        // });

        // xit('write and read variable different server', async () => {
        //     await raftClient.setVar('foo', 42);
        //     expect(await raftClient.getVar('foo')).toBe(42);
        // });
    });



}

