# Node architecture

The node consists of two main components: RPC and database. Combined, a simple system supports the core mechanism of the transport layer: the node serves public RPC requests, while using the database to store the notes associated with the requests.

While currently only supporting a centralized architecture, it is expected to evolve into a more distributed approach in order to increase the resilience of the transport layer.

## RPC

The RPC component provides a public gRPC API with which users can send and fetch notes.
Requests are processed and then proxied to the database.

Note streaming is also supported through gRPC.

This is the _only_ externally facing component.

## Database

The database is responsible for storing the private notes.
As the transport layer was built with a focus on user privacy, no user data is stored.

Notes are stored for a predefined duration (default at 30 days).
An internal sub-component running in the node is responsible for the database maintenance, performing the removal of expired notes.

Currently, SQLite is the only database implementation provided.
