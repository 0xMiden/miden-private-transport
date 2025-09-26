# Configuration and Usage

Configuration and operation of the Miden Transport Layer node is simple.


## Operation

Start the node with the desired public gRPC server address.
For example,

```sh
miden-private-transport-node-bin \
  --host 0.0.0.0 \
  --port 9730 \
  --database-url mtln.db
```

> [!NOTE]
> `miden-private-transport-node-bin` provides default arguments aimed at development.

Configuration is purely made using command line arguments. Run `miden-private-transport-node-bin --help` for available options.

If using the provided Docker setup, see the [setup page](installation.md#docker-setup). Configure the node binary launch arguments accordingly before starting Docker containers.
