
import { times } from 'lodash';

import { RaftNodeProcesses, startRaftNode } from './testUtil';
import { NotFoundError } from './api';

describe('integration', () => {

    let raftNodes: RaftNodeProcesses[];

    beforeEach(async () => {
        raftNodes = times(3).map(index => startRaftNode(index));
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
        expect((await raftNodes[0].api.getState()).role).toBe('Follower');
        expect((await raftNodes[1].api.getState()).role).toBe('Follower');
        expect((await raftNodes[2].api.getState()).role).toBe('Follower');
    });
});

