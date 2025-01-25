# raft
A toy implementation of the Raft consensus algorithm

## Get started
Install [devcontainer](https://code.visualstudio.com/docs/devcontainers/containers).

Build the project
```
yarn build
```

Build and run Raft server
```
yarn build-raft-server
ID=0 PORT=8000 RPC_PORT=50000 NUM_NODES=3 yarn start-raft-server
```

Run unit tests
```
yarn test-raft
```

Run integration tests
```
yarn build-raft-server
yarn test-integration
```

## TODO:
- [x] Implement AppendEntries RPC
- [x] The leader commits values (sending update to majority of followers) before applying to the state machine
- [ ] Store the RPC client in a structure instead of creating a new one every time
- [x] The nodes handle "Error: transport error" when nodes are down
- [x] The setVar call should succeed when some (not majority) of followers are down
- [ ] Send partial entry updates rather than all entries every time
- [x] Depose leader when AppendEntries returns a higer term
- [ ] Clean up the persisted state on disk after every integration test
- [ ] Every client operation should have a unique ID and be idempotent, so it can be retried if it fails
- [ ] If log is detected corrupted, reset the state and start with an empty log
- [ ] Append to the log in such a way that is possible to know when it's persisted

# Learnings
- Raft protocol implementation
- Improve Rust skills
- Structure a Rust project with multiple targets (WASM library, other binaries)
- Use gRPC in Rust
- Write meaningful tests for a distributed system
- Use devcontainer
- Concurrency in Rust: RwLock, Arc and state shared among threads
