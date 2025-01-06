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
- [ ] The leader commits values (sending update to majority of followers) before applying to the state machine
- [ ] Store the RPC client in a structure instead of creating a new one every time

# Learnings
- Raft protocol implementation
- Improve Rust skills
- Structure a Rust project with multiple targets (WASM library, other binaries)
- Use gRPC in Rust
- Write meaningful tests for a distributed system
- Use devcontainer
