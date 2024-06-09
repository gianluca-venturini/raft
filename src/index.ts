import { greet } from '../raft/pkg/raft';

console.log('test');
console.log('test');

async function run() {
    console.log(greet('World'));
}

run();