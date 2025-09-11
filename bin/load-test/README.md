# Miden Private Transport Node Load-Test Tool

Tests the node implementation by flooding it with different requests.
Success rate, latency, and throughput are measured for each testing scenario.

## Scenarios

- `send-note`: Issue "SendNote" requests (one note) to the server;
- `fetch-notes`: Issue "FetchNotes" requests to the server (responses will have `n`-configured notes);
- `mixed`: Issue "SendNote" + "FetchNotes" requests in random order. "FetchNotes" may yield some response notes;
- `req-rep`: Issue one "SendNote" to one "FetchNotes" requests. "FetchNotes" response will yield one note.
