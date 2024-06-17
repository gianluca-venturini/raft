# raft
A toy implementation of the Raft consensus algorithm

## Get started
Install dependencies
```
brew install cmake
brew install protobuf
```

Build the project
```
yarn build
```

Build and run Raft server
```
yarn build-raft-server
PORT=8000 RPC_PORT=50000 yarn start-raft-server
```

Run integration tests
```
yarn build-raft-server
yarn test-integration
```

# Learnings
- Raft protocol implementation
- Improve Rust skills
- Structure a Rust project with multiple targets (WASM library, other binaries)
- Use gRPC in Rust
- Write meaningful tests for a distributed system
