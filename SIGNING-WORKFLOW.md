# FROST Transaction Signing & Broadcasting Workflow

This guide walks you through signing Mina transactions using FROST threshold signatures and broadcasting them to the network.

## ⚠️ Security Warning

**The `frost-bluepallas` crate, along with the `bluepallas_compat` and `pallas_message` modules of `mina-tx`, have undergone a security audit, available [here](audits/VAR_Nori_260615_multisig_V2.pdf). The rest of the repository (including the remainder of `mina-tx` and all of `mina-frost-client`) was not covered by the audit. Even audited code may still contain bugs and security vulnerabilities. Use it at your own risk. The authors and contributors take no responsibility for any loss or damage resulting from the use of this code.**

---

## Quick Reference

| Task | Command |
|------|---------|
| Initialize config | `mina-frost-client init -c <CONFIG_PATH>` |
| Export contact | `mina-frost-client export -n <NAME> -c <CONFIG_PATH>` |
| Import contact | `mina-frost-client import <CONTACT_STRING> -c <CONFIG_PATH>` |
| Start DKG (coordinator) | `mina-frost-client dkg -d <DESC> -s <SERVER_URL> -t <THRESHOLD> -S <PUBKEYS> -c <CONFIG_PATH>` |
| Join DKG (participant) | `mina-frost-client dkg -d <DESC> -s <SERVER_URL> -t <THRESHOLD> -c <CONFIG_PATH>` |
| List groups | `mina-frost-client groups -c <CONFIG_PATH>` |
| Coordinate signing | `mina-frost-client coordinator -g <GROUP_PUBKEY> -S <SIGNER_PUBKEYS> -m <TX_FILE> -o <SIG_OUT> -n <NETWORK> -c <CONFIG_PATH>` |
| Join signing | `mina-frost-client participant -g <GROUP_PUBKEY> -S <SESSION_ID> -c <CONFIG_PATH>` |
| Build GraphQL | `mina-frost-client graphql-build -i <INPUT_JSON> -o <OUTPUT_FILE>` |
| Broadcast | `mina-frost-client graphql-broadcast -g <GRAPHQL_FILE> -e <ENDPOINT_URL>` |

---

## Prerequisites

### Software Requirements

| Software | Version | Install Command |
|----------|---------|-----------------|
| Rust | 1.87.0+ | See [the Rust book](https://doc.rust-lang.org/book/ch01-01-installation.html) |
| mina-frost-client | latest | `cargo install --git https://github.com/Raspberry-Devs/mina-multi-sig.git --locked mina-frost-client --features mesa-hardfork` (leave the feature out for a network that does not run the Mesa hard fork) |
| frostd | latest | `cargo install --git https://github.com/ZcashFoundation/frost-zcash-demo.git --locked frostd` |
| mkcert | latest | `apt install mkcert` |

---

## Section 1: Initial Setup

### 1.1 Install Tools

```bash
# Install mina-frost-client with the transaction rules of the Mesa hard fork, which Mainnet runs.
# Leave `--features mesa-hardfork` out for a network that does not run Mesa.
cargo install --git https://github.com/Raspberry-Devs/mina-multi-sig.git --locked mina-frost-client --features mesa-hardfork

# Install frostd server
cargo install --git https://github.com/ZcashFoundation/frost-zcash-demo.git --locked frostd
```

### 1.2 Initialize Configuration

Each participant must initialize their own configuration file:

```bash
mina-frost-client init -c <CONFIG_PATH>
```

**Example:**
```bash
mina-frost-client init -c ~/.frost/alice.toml
```

> **Note:** The config file contains private FROST shares in clear text. Keep it safe and never share it with anyone.

### 1.3 Exchange Contacts

Participants must exchange contact information before DKG or signing sessions.

**Export your contact:**
```bash
mina-frost-client export -n <YOUR_NAME> -c <CONFIG_PATH>
```

**Import another participant's contact:**
```bash
mina-frost-client import <CONTACT_STRING> -c <CONFIG_PATH>
```

**List all contacts:**
```bash
mina-frost-client contacts -c <CONFIG_PATH>
```

#### Sharing Contacts
The FROST tool does not support a way for participants to share contact information, instead we recommend using your favourite messenger application (e.g. WhatsApp, Signal, Telegram) to share contact strings.

---

## Section 2: Server Setup

The FROST protocol requires a coordination server (`frostd`) for participants to communicate.

### 2.1 Generate TLS Certificates

#### Development (mkcert)

```bash
mkcert localhost 127.0.0.1 ::1 2>/dev/null
```

This creates `localhost+2.pem` and `localhost+2-key.pem` in the current directory, this will be installed to be trusted by your local system trust store. This is not recommended for production systems.

### Production
We recommend setting up a DNS name which points to your `frostd` instance and generating trusted certificates through a certificate authority such as [Let's Encrypt](https://letsencrypt.org/). This is out of the scope of this document, and we recommend looking at Let's Encrypt's documentation.

### 2.2 Start the Server

```bash
frostd --tls-cert <CERT.pem> --tls-key <KEY.pem>
```

The server runs on `localhost:2744` by default.

For production systems, we recommend using a reverse proxy (such as nginx) with a domain name. Additionally, you need a central authority to sign your certificates as explained above.

For more information, see the [frostd documentation](https://frost.zfnd.org/zcash/server.html).

---

## Section 3: Key Generation

Choose **one** of the following methods to generate group keys.

### Option A: Distributed Key Generation (DKG) — Recommended

DKG distributes key generation across all participants, with no single party ever knowing the complete private key.

**Coordinator** (one participant initiates with `-S` flag listing other participants' public keys):

```bash
mina-frost-client dkg \
  -c <CONFIG_PATH> \
  -d "<GROUP_DESCRIPTION>" \
  -s <SERVER_URL> \
  -t <THRESHOLD> \
  -S <PARTICIPANT_PUBKEY_1>,<PARTICIPANT_PUBKEY_2>
```

**Other Participants** (join without `-S` flag):

```bash
mina-frost-client dkg \
  -c <CONFIG_PATH> \
  -d "<GROUP_DESCRIPTION>" \
  -s <SERVER_URL> \
  -t <THRESHOLD>
```

**Example (2-of-3 threshold):**

```bash
# Alice (coordinator)
mina-frost-client dkg \
  -c ~/.frost/alice.toml \
  -d "Alice, Bob and Eve's group" \
  -s localhost:2744 \
  -t 2 \
  -S <BOB_PUBKEY>,<EVE_PUBKEY>

# Bob (participant)
mina-frost-client dkg \
  -c ~/.frost/bob.toml \
  -d "Alice, Bob and Eve's group" \
  -s localhost:2744 \
  -t 2

# Eve (participant)
mina-frost-client dkg \
  -c ~/.frost/eve.toml \
  -d "Alice, Bob and Eve's group" \
  -s localhost:2744 \
  -t 2
```

> **Important:** All participants must use the **same description** and **threshold** values.

### Option B: Trusted Dealer — Test Only

⚠️ **Warning:** This method is for testing only. The trusted dealer knows all key shares, which defeats the purpose of threshold signatures.

```bash
mina-frost-client trusted-dealer \
  -c <CONFIG_PATH_1> -c <CONFIG_PATH_2> -c <CONFIG_PATH_3> \
  -d "<GROUP_DESCRIPTION>" \
  -N <NAME_1>,<NAME_2>,<NAME_3> \
  -t <THRESHOLD>
```

**Example:**
```bash
mina-frost-client trusted-dealer \
  -c alice.toml -c bob.toml -c eve.toml \
  -d "Test group" \
  -N Alice,Bob,Eve \
  -t 2
```

### 3.3 Verify Group Creation

After key generation, verify the group was created:

```bash
mina-frost-client groups -c <CONFIG_PATH>
```

Note the **group public key** — you'll need it for signing sessions.

---

## Section 4: Transaction Preparation Overview

Before signing, you need an unsigned transaction in JSON format. This section shows how to generate transactions using o1js. See [the O1JS workflow document](O1JS-WORKFLOW.md) for full project setup.

### 4.1 Project Setup

Create a new Node.js project for transaction generation:

```bash
mkdir mina-tx-generator && cd mina-tx-generator
npm init -y
npm install o1js
npm install -D typescript ts-node @types/node
```

**tsconfig.json:**
```json
{
  "compilerOptions": {
    "target": "es2021",
    "module": "nodenext",
    "moduleResolution": "nodenext",
    "strict": true,
    "esModuleInterop": true,
    "experimentalDecorators": true,
    "emitDecoratorMetadata": true,
    "outDir": "./dist"
  },
  "include": ["src/**/*"]
}
```

Create an output directory:
```bash
mkdir -p tx-json
```

### 4.2 ZKApp Transactions (Primary)

ZKApp transactions are the primary use case for FROST multi-sig on Mina. The key pattern is using `Permissions.signature()` and `this.requireSignature()` to enable signature-based authorization.

#### 4.2.1 State Update Contract

This contract stores on-chain state and requires a signature to modify it:

**src/update_state.ts:**
```typescript
import {
  SmartContract,
  State,
  state,
  method,
  Field,
  Permissions,
  Mina,
  AccountUpdate,
  PublicKey,
} from 'o1js';
import * as fs from 'fs';
import { UpdateFullCommitment } from './commit';

const MINA_TESTNET_URL = 'https://api.minascan.io/node/devnet/v1/graphql';
const FEE = 100_000_000; // 0.1 MINA
const DEPLOYER_KEY = '<PUBLIC_KEY>';

class StateContract extends SmartContract {
  @state(Field) counter = State<Field>();

  init() {
    super.init();
    this.counter.set(Field(0));
    this.account.permissions.set({
      ...Permissions.default(),
      editState: Permissions.signature(),
    });
  }

  @method async incrementCounter() {
    this.requireSignature();
    const currentValue = this.counter.get();
    this.counter.requireEquals(currentValue);
    this.counter.set(currentValue.add(1));
  }
}

async function generateUpdateStateTx() {
  // 1. Setup Mina testnet
  const network = Mina.Network(MINA_TESTNET_URL);
  Mina.setActiveInstance(network);

  // 2. Get accounts
  const deployer = PublicKey.fromBase58(DEPLOYER_KEY);
  const contractAccount = PublicKey.fromBase58(DEPLOYER_KEY);
  const contract = new StateContract(contractAccount);

  // 3. Compile (needed for verification key)
  await StateContract.compile();

  // 4. Create deploy transaction (unsigned)
  const deployTx = await Mina.transaction(
    { sender: deployer, fee: FEE },
    async () => {
      await contract.deploy();
    }
  );

  // 5. Create update transaction (unsigned)
  const tx = await Mina.transaction(
    { sender: deployer, fee: FEE },
    async () => {
      await contract.incrementCounter();
    }
  );

  UpdateFullCommitment(deployTx, tx);

  fs.writeFileSync('./tx-json/deploy-state-contract.json', deployTx.toJSON());
  console.log('Deploy transaction saved to ./tx-json/deploy-state-contract.json');

  fs.writeFileSync('./tx-json/update-state-transaction.json', tx.toJSON());
  console.log('Update transaction saved to ./tx-json/update-state-transaction.json');
}

generateUpdateStateTx();
```

Run with:
```bash
npx ts-node src/update_state.ts
```
#### 4.2.2 Verification Key Update Contract

This contract allows updating its verification key (useful after Mina hard forks):

### src/update_verification_key.ts
```typescript
import {
  SmartContract,
  VerificationKey,
  method,
  Permissions,
  Mina,
  AccountUpdate,
  PublicKey,
} from 'o1js';
import * as fs from 'fs';
import { UpdateFullCommitment } from './commit';

const MINA_TESTNET_URL = 'https://api.minascan.io/node/devnet/v1/graphql';
const FEE = 100_000_000; // 0.1 MINA
const DEPLOYER_KEY = '<PUBLIC_KEY>';

class UpdatableContract extends SmartContract {
  init() {
    super.init();
    this.account.permissions.set({
      ...Permissions.default(),
      setVerificationKey: Permissions.VerificationKey.signature(),
    });
  }

  @method async updateVerificationKey(verificationKey: VerificationKey) {
    this.requireSignature();
    this.account.verificationKey.set(verificationKey);
  }
}

class NewContract extends SmartContract {
  @method async dummy() {
    // Different contract = different verification key
  }
}

async function generateUpdateVerificationKeyTx() {
  // 1. Setup Mina testnet
  const network = Mina.Network(MINA_TESTNET_URL);
  Mina.setActiveInstance(network);

  // 2. Get accounts
  const deployer = PublicKey.fromBase58(DEPLOYER_KEY);
  const contractAccount = PublicKey.fromBase58(DEPLOYER_KEY);
  const contract = new UpdatableContract(contractAccount);

  // 3. Compile original contract (needed for verification key)
  await UpdatableContract.compile();

  // 4. Create deploy transaction (unsigned)
  const deployTx = await Mina.transaction(
    { sender: deployer, fee: FEE },
    async () => {
      await contract.deploy();
    }
  );

  // 5. Compile new contract for different verification key
  const { verificationKey: newVerificationKey } = await NewContract.compile();

  // 6. Create update transaction (unsigned)
  const tx = await Mina.transaction(
    { sender: deployer, fee: FEE },
    async () => {
      await contract.updateVerificationKey(newVerificationKey);
    }
  );

  UpdateFullCommitment(deployTx, tx);

  fs.writeFileSync('./tx-json/deploy-updatable-contract.json', deployTx.toJSON());
  console.log('Deploy transaction saved to ./tx-json/deploy-updatable-contract.json');

  fs.writeFileSync('./tx-json/update-verification-key-transaction.json', tx.toJSON());
  console.log('Update transaction saved to ./tx-json/update-verification-key-transaction.json');
}

generateUpdateVerificationKeyTx();
```

#### Transaction Structure

The generated JSON has this structure:

```json
{
  "feePayer": {
    "body": {
      "publicKey": "<FEE_PAYER_PUBLIC_KEY>",
      "fee": "100000000",
      "nonce": "0",
      "validUntil": null
    },
    "authorization": ""
  },
  "accountUpdates": [
    {
      "body": {
        "publicKey": "<CONTRACT_PUBLIC_KEY>",
        "balanceChange": { "magnitude": "0", "sgn": "Positive" },
        ...
      },
      "authorization": {
        "signature": null
      }
    }
  ],
  "memo": ""
}
```

#### Signature Injection

FROST signatures are injected into the transaction at:

1. **Fee Payer** — `feePayer.authorization` (always required)
2. **Account Updates** — `accountUpdates[*].authorization.signature` (when `requireSignature()` is called)

| Transaction Type | Signatures Required |
|-----------------|---------------------|
| Deploy | Fee payer + Contract account |
| State update | Fee payer + Contract account (if `requireSignature()` used) |
| Permissions update | Fee payer + Contract account (if `requireSignature()` used) |
| Verification key update | Fee payer + Contract account (if `requireSignature()` used) |

#### Transactions with Full Commitments
Note that the FROST multi-sig does not sign partial account updates (updates whose `use_full_commitment` field is `false`).

#### Nonce Management

Query the current nonce for your FROST group account using GraphQL:

```graphql
query {
  accounts(publicKey: "<GROUP_PUBLIC_KEY_ADDR>") {
    nonce
  }
}
```

Or use the Mina CLI:
```bash
mina account get --public-key <GROUP_PUBLIC_KEY_ADDR>
```

When generating transactions, set the correct nonce:

```typescript
const tx = await Mina.transaction(
  { sender: frostGroupPubKey, fee: 1e8, nonce: 5 },  // Set nonce explicitly
  async () => {
    // ... transaction logic
  }
);
```

### 4.3 Legacy Payment Transactions

For simple payment transactions without zkApp functionality, create a JSON file manually:

**tx-json/payment.json:**
```json
{
  "to": "<RECEIVER_ADDRESS>",
  "from": "<GROUP_PUBLIC_KEY>",
  "fee": "100000000",
  "amount": "1000000000",
  "nonce": "0",
  "memo": "FROST payment",
  "valid_until": "4294967295",
  "tag": [false, false, false]
}
```

| Field | Description |
|-------|-------------|
| `to` | Recipient's Mina address (B62...) |
| `from` | Sender's Mina address (your FROST group public key) |
| `fee` | Transaction fee in nanomina (100000000 = 0.1 MINA) |
| `amount` | Transfer amount in nanomina (1000000000 = 1 MINA) |
| `nonce` | Sender's current account nonce |
| `memo` | Optional memo (max 32 characters) |
| `valid_until` | Slot number until which transaction is valid (max = 4294967295) |
| `tag` | Transaction type flags `[false, false, false]` for payments |

### 4.4 Key Technical Notes

- **`proofsEnabled: false`** — Transactions use signatures instead of zero-knowledge proofs
- **`Permissions.signature()`** — Allows authorization via signature instead of proof
- **`this.requireSignature()`** — Method call that requires contract account signature
- **Compilation is still required** — Generates verification key needed for deployment
- **Fee payer must have funds** — Ensure your FROST group account is funded before signing

---

## Section 5: Signing Session

### 5.1 Coordinator Starts Session

The coordinator initiates the signing session with the transaction file:

```bash
mina-frost-client coordinator \
  -c <CONFIG_PATH> \
  -s <SERVER_URL> \
  -g <GROUP_PUBLIC_KEY> \
  -S <SIGNER_PUBKEY_1>,<SIGNER_PUBKEY_2> \
  -m <TRANSACTION_FILE> \
  -o <SIGNATURE_OUTPUT> \
  -n <NETWORK>
```

**Parameters:**
| Flag | Description |
|------|-------------|
| `-c` | Path to coordinator's config file |
| `-s` | Server URL (optional if stored in group config) |
| `-g` | Group public key (from `groups` command) |
| `-S` | Comma-separated public keys of signers to include |
| `-m` | Path to unsigned transaction JSON file |
| `-o` | Output path for signature (use `-` for stdout) |
| `-n` | Network: `mainnet` or `testnet` (default: `testnet`) |

**Example:**
```bash
mina-frost-client coordinator \
  -c ~/.frost/alice.toml \
  -s localhost:2744 \
  -g <GROUP_PUBLIC_KEY> \
  -S <BOB_PUBKEY>,<EVE_PUBKEY> \
  -m ./tx-json/update-state-transaction.json \
  -o ./signed-tx.json \
  -n testnet
```

### 5.2 Participants Join Session

Each selected signer joins the session:

```bash
mina-frost-client participant \
  -c <CONFIG_PATH> \
  -s <SERVER_URL> \
  -g <GROUP_PUBLIC_KEY> \
  -S <SESSION_ID> \
  -y
```

**Parameters:**
| Flag | Description |
|------|-------------|
| `-c` | Path to participant's config file |
| `-s` | Server URL (optional if stored in group config) |
| `-g` | Group public key |
| `-S` | Session ID (use `sessions` command to list if coordinator output was lost) |
| `-y` | Auto-approve signing (skip confirmation prompt) |

**Example:**
```bash
# Bob joins
mina-frost-client participant \
  -c ~/.frost/bob.toml \
  -s localhost:2744 \
  -g <GROUP_PUBLIC_KEY> \
  -S <SESSION_ID> \
  -y

# Eve joins
mina-frost-client participant \
  -c ~/.frost/eve.toml \
  -s localhost:2744 \
  -g <GROUP_PUBLIC_KEY> \
  -S <SESSION_ID> \
  -y
```

### 5.3 Session Management

**List active sessions:**
```bash
mina-frost-client sessions \
  -c <CONFIG_PATH> \
  -s <SERVER_URL> \
  -g <GROUP_PUBLIC_KEY>
```

**Close all sessions (cleanup):**
```bash
mina-frost-client sessions \
  -c <CONFIG_PATH> \
  -s <SERVER_URL> \
  -g <GROUP_PUBLIC_KEY> \
  --close-all
```

Note: All users must be online during FROST signing for successful participation, if a user loses connection, the session must be restarted.

---

## Section 6: Broadcasting

### 6.1 Build GraphQL Mutation

Convert the signed transaction to a GraphQL mutation:

```bash
mina-frost-client graphql-build \
  -i <SIGNED_TRANSACTION_JSON> \
  -o <GRAPHQL_OUTPUT_FILE>
```

**Example:**
```bash
mina-frost-client graphql-build \
  -i ./signed-tx.json \
  -o ./broadcast.graphql
```

### 6.2 Broadcast to Network

Submit the GraphQL mutation to a Mina node:

```bash
mina-frost-client graphql-broadcast \
  -g <GRAPHQL_FILE> \
  -e <ENDPOINT_URL>
```

**Example:**
```bash
mina-frost-client graphql-broadcast \
  -g ./broadcast.graphql \
  -e https://api.minascan.io/node/devnet/v1/graphql
```

### GraphQL Endpoints

These are an example of GraphQL endpoints, we highly recommending users to use their own node's URLs if they have one.

| Network | Endpoint |
|---------|----------|
| Mainnet | `https://api.minascan.io/node/mainnet/v1/graphql` |
| Devnet | `https://api.minascan.io/node/devnet/v1/graphql` |
| Berkeley | `https://api.minascan.io/node/berkeley/v1/graphql` |

---

## Command Reference

| Command | Description | Key Flags |
|---------|-------------|-----------|
| `init` | Initialize participant config | `-c <config>` |
| `export` | Export contact string | `-n <name>` `-c <config>` |
| `import` | Import a contact | `<contact>` `-c <config>` |
| `contacts` | List contacts | `-c <config>` |
| `remove-contact` | Remove a contact | `-c <config>` `-p <pubkey>` |
| `trusted-dealer` | Test-only key generation | `-c <configs...>` `-d <desc>` `-N <names>` `-t <threshold>` |
| `dkg` | Distributed key generation | `-c <config>` `-d <desc>` `-s <server>` `-t <threshold>` `-S <participants>` |
| `groups` | List groups | `-c <config>` |
| `remove-group` | Remove a group | `-c <config>` `-g <group>` |
| `sessions` | List/manage sessions | `-c <config>` `-s <server>` `-g <group>` `--close-all` |
| `coordinator` | Start signing session | `-c <config>` `-s <server>` `-g <group>` `-S <signers>` `-m <message>` `-o <signature>` `-n <network>` |
| `participant` | Join signing session | `-c <config>` `-s <server>` `-g <group>` `-S <session>` `-y` |
| `graphql-build` | Build GraphQL mutation | `-i <input>` `-o <output>` |
| `graphql-broadcast` | Broadcast transaction | `-g <graphql>` `-e <endpoint>` |

---

## Troubleshooting

### Common FROST Tool Issues

| Issue | Possible Cause | Solution |
|-------|---------------|----------|
| Connection refused | Server not running | Start `frostd` server |
| Certificate error | Invalid TLS cert | Regenerate with `mkcert` or look at your certificate setup |
| Group not found | Wrong group key | Run `groups` to list valid keys |
| Session timeout | Participants too slow | Setup a new session |


### Common Mina Blockchain Issues
| Issue | Possible Cause | Solution |
|-------|---------------|----------|
| Invalid Fee Excess | Fee Payer does not have any funds | Ensure that the FROST account you generate has funds or use an external FeePayer |

## See Also

- [O1JS-WORKFLOW.md](./O1JS-WORKFLOW.md) — Generating unsigned Mina transactions with o1js
- [README.md](./README.md) — Project overview and repository layout
- [frostd documentation](https://frost.zfnd.org/zcash/server.html) — FROST server setup guide
- [Mina Protocol Documentation](https://docs.minaprotocol.com/) — Official Mina documentation
