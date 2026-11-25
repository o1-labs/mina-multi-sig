# Mina Multi-Sig

[![Rust](https://github.com/raspberry-devs/mina-multi-sig/actions/workflows/rust.yml/badge.svg)](https://github.com/raspberry-devs/mina-multi-sig/actions/workflows/rust.yml)
[![Documentation](https://github.com/raspberry-devs/mina-multi-sig/actions/workflows/docs.yml/badge.svg)](https://github.com/raspberry-devs/mina-multi-sig/actions/workflows/docs.yml)

This repository provides an experimental implementation of multi-signature tooling for the [Mina Protocol](https://minaprotocol.com/). The project is built around [FROST](https://github.com/cfrg/draft-irtf-cfrg-frost) (Flexible Round-Optimized Schnorr Threshold signatures) and contains both a reusable library and a command line client.

## ⚠️ Security Warning

**The `frost-bluepallas` crate, along with the `bluepallas_compat` and `pallas_message` modules of `mina-tx`, have undergone a security audit, available [here](audits/VAR_Nori_260615_multisig_V2.pdf). The rest of the repository (including the remainder of `mina-tx` and all of `mina-frost-client`) was not covered by the audit. Even audited code may still contain bugs and security vulnerabilities. Use it at your own risk. The authors and contributors take no responsibility for any loss or damage resulting from the use of this code.**

## Why Multi‑Sig on Mina?

Besides the usual advantages of shared control over accounts, threshold signatures help mitigate issues around how the Mina protocol handles hard forks. During a fork zero knowledge verification keys may become obselete which causes all smart contracts to fallback to using signature-based updates. Multi‑sig coordination provides an additional layer of safety when updating verification keys after a hard fork.

## Repository Layout

- **`frost-bluepallas/`** – A Rust crate implementing FROST for Mina's Pallas curve using the Poseidon hash function. It allows generation of signatures that are compatible with Mina nodes and includes example programs for key generation and transaction signing.
- **`mina-tx/`** – A library for working with Mina transactions, including support for signing creating graphql querries.
- **`mina-frost-client/`** – A demo client and utilities for running distributed key generation and signing sessions. It exposes various sub‑commands for initializing participants, running a trusted dealer or DKG, and coordinating signing rounds.

## API Documentation

- Main docs: <https://raspberry-devs.github.io/mina-multi-sig/frost_bluepallas/>
- Other crates in the workspace are available from the rustdoc sidebar.

## Signing Workflow

A document which describes a full signing workflow with the FROST tool can be read [here](SIGNING-WORKFLOW.md). This provides a comprehensive step-by-step tutorial on how to use the tool and to submit FROST-signed transactions to the Mina blockchain.

Additionally, view the [transaction generation document](O1JS-WORKFLOW.md) for example scripts which specify transaction JSON generation to use in the FROST Multi-sig.

## Installation

### mina-frost-client

To install `mina-frost-client` for a network that runs the Mesa hard fork, as Mainnet does, run

```bash
cargo install --git https://github.com/Raspberry-Devs/mina-multi-sig.git --locked mina-frost-client --features mesa-hardfork
```

Without `--features mesa-hardfork` the client follows the transaction rules from before Mesa (8 app state fields, transaction version 3). Such a build cannot read a zkApp transaction made for a Mesa network, and a Mesa build cannot read one made for an earlier network. Config files and key shares are the same for both builds.

### frost-bluepallas

To use `frost-bluepallas` as a dependency in your Rust project, add this to your `Cargo.toml`:

```toml
[dependencies]
frost-bluepallas = { git = "https://github.com/Raspberry-Devs/mina-multi-sig.git" }
```

You can also specify a exact commit on the main repository you would like to use

```toml
[dependencies]
frost-bluepallas = { git = "https://github.com/Raspberry-Devs/mina-multi-sig.git", rev = "commit_hash" }
```

## Example Usage

Below is a minimal outline of how the client can be used. See the `examples/` folders in each crate for complete scripts.

### Trusted Dealer Setup

This command is **test-only**.

```bash
# Initialize participant configurations
cargo run --bin mina-frost-client -- init -c alice.toml
cargo run --bin mina-frost-client -- init -c bob.toml
cargo run --bin mina-frost-client -- init -c eve.toml

# Generate key shares with the trusted dealer helper
cargo run --bin mina-frost-client -- trusted-dealer \
  -d "Alice, Bob and Eve's group" \
  --names Alice,Bob,Eve \
  --threshold 2 \
  -c alice.toml -c bob.toml -c eve.toml
```

### Running the Server

Install `frostd` using cargo with

```bash
cargo install --git https://github.com/ZcashFoundation/frost-zcash-demo.git --locked frostd
```

Generate certificates for the server with `mkcert`

```bash
mkcert localhost 127.0.0.1 ::1 2>/dev/null
```

Start the server

```bash
frostd --tls-cert localhost+2.pem --tls-key localhost+2-key.pem
```

More information on how to run a server and set it up for production can be found [here](https://frost.zfnd.org/zcash/server.html)

### Distributed Key Generation (DKG)

```bash
# Initialize configurations and exchange contacts
cargo run --bin mina-frost-client -- init -c alice.toml
cargo run --bin mina-frost-client -- export --name 'Alice' -c alice.toml
cargo run --bin mina-frost-client -- import -c alice.toml <contact_string>

# Start DKG process (coordinator)
cargo run --bin mina-frost-client -- dkg \
  -d "Alice, Bob and Eve" \
  -s localhost:2744 \
  -S <BOB_PUBLIC_KEY>,<EVE_PUBLIC_KEY> \
  -t 2 -c alice.toml

# Each participant joins the DKG
cargo run --bin mina-frost-client -- dkg \
  -d "Alice, Bob and Eve" \
  -s localhost:2744 \
  -t 2 -c bob.toml
```

### View Groups

```bash
# Each user can view group information
cargo run --bin mina-frost-client -- groups \
  -c alice.toml
```

### Signing Session

```bash
# Start a signing session (coordinator)
cargo run --bin mina-frost-client -- coordinator \
  -c alice.toml \
  --server-url localhost:2744 \
  --group <GROUP_PUBLIC_KEY> \
  -S <BOB_PUBLIC_KEY>,<EVE_PUBLIC_KEY> \
  -m message.txt -o signature.hex

# Each participant joins the session
cargo run --bin mina-frost-client -- participant \
  -c bob.toml \
  --server-url localhost:2744 \
  --group <GROUP_PUBLIC_KEY>
```

## Example Transaction Message

```json
{
  "to": "B62qkcvM4DZE7k23ZHMLt1uaMVcixuxxuyz1XNJNCLkFbitDdUHxWs1",
  "from": "B62qkcvM4DZE7k23ZHMLt1uaMVcixuxxuyz1XNJNCLkFbitDdUHxWs1",
  "fee": "1000000000",
  "amount": "1000000000",
  "nonce": "1",
  "memo": "Hello Mina x FROST from the Rasp",
  "valid_until": "4294967295",
  "tag": [false, false, false]
}
```

## Contributing

Contributions are welcome! Feel free to open issues or pull requests. Please note that all code is licensed under the [Apache-2.0](LICENSE) license.

## License

This project is distributed under the terms of the Apache License, Version 2.0. See the [LICENSE](LICENSE) file for details.
