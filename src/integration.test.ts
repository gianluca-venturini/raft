
import { times } from 'lodash';

import { RaftNodeProcesses, startRaftNode } from './testUtil';
import { NotFoundError, getVar, setVar } from './api';

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
        // Tests if nodes spin up and down correctly
        await new Promise(resolve => setTimeout(resolve, 200));
    });

    it('read variable not found', async () => {
        await expect(getVar('foo')).rejects.toThrow(NotFoundError);
    });

    it('write and read variable', async () => {
        await setVar('foo', 42);
        expect(await getVar('foo')).toBe(42);
    });
});

