import { greet } from '../raft/client/pkg/raft_client.js';

console.log('test');
console.log('test');

async function run() {
    console.log(greet('World'));
}

run();