
import { times } from 'lodash';

import { RaftNodeProcesses, startRaftNode } from './testUtil';
import { NotFoundError } from './api';

// describe('integration 3 nodes', () => {
//     integrationTestsWithNodes(3);
// });

describe('integration 11 nodes', () => {
    integrationTestsWithNodes(11);
});

function integrationTestsWithNodes(numNodes: number) {
    let raftNodes: RaftNodeProcesses[];

    beforeEach(async () => {
        raftNodes = times(numNodes).map(index => startRaftNode(index, numNodes));
        await Promise.all(raftNodes.map(server => server.started));
    });

    afterEach(async () => {
        await Promise.all(raftNodes.map(server => server.exit()));
    });

    it('do nothing', async () => {
        // Tests if nodes spin up and down correctly in beforeEach() and afterEach()
        await new Promise(resolve => setTimeout(resolve, 200));
    });

    it('read variable not found', async () => {
        await expect(raftNodes[0].api.getVar('foo')).rejects.toThrow(NotFoundError);
    });

    it('write and read variable', async () => {
        await raftNodes[0].api.setVar('foo', 42);
        expect(await raftNodes[0].api.getVar('foo')).toBe(42);
    });

    it('all nodes are followers at the beginning', async () => {
        for (const node of raftNodes) {
            expect((await node.api.getState()).role).toBe('Follower');
        }
    });

    fit('one node is elected leader', async () => {
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
}

