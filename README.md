# Miden Transport Layer for Private Notes

<!--`TODO(template) update badges`-->
[![LICENSE](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/0xMiden/project-template/blob/main/LICENSE)
[![test](https://github.com/0xMiden/project-template/actions/workflows/test.yml/badge.svg)](https://github.com/0xMiden/project-template/actions/workflows/test.yml)
[![RUST_VERSION](https://img.shields.io/badge/rustc-1.85+-lightgray.svg)](https://www.rust-lang.org/tools/install)
[![crates.io](https://img.shields.io/crates/v/miden-mybinary)](https://crates.io/crates/miden-mybinary)

## Overview

### Telemetry

Metrics and Traces are provided for the Node implementation.
Data is exported using OpenTelemetry.
A Docker-based setup is provided, with the following stack:
- OpenTelemetry Collector;
- Tempo (Traces);
- Prometheus (Metrics);
- Grafana (Visualization).

### Crates

- `client`: Client implementation.
- `node`: Node/server implementation.
- `proto`: Protobuf definitions and generated code;

## Quick Start

## API Reference

## Usage

### Sending a Note

### Receiving Notes

## Contributing

At minimum, please see our [contributing](https://github.com/0xMiden/.github/blob/main/CONTRIBUTING.md) guidelines and our [makefile](Makefile) for example workflows
e.g. run the testsuite using

```sh
make test
```

Note that we do _not_ accept low-effort contributions or AI generated code. For typos and documentation errors please
rather open an issue.

## License
This project is [MIT licensed](./LICENSE).
