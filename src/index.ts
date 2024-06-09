import { greet } from '../raft_server/pkg';

console.log('test');
console.log('test');

async function run() {
    console.log(greet('World'));
}

run();