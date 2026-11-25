//! A zkApp command taken from Mina Mainnet after the Mesa hard fork.
//!
//! `tests/data/mainnet/zkapp-575805.json` is the command
//! `5JuXLrNsd4Qk4mnt5ovr3fJugpvw9z5mNsUk8ziZnGToGHCWxuSa`, applied in block 575,805, as a
//! node's GraphQL API returns it under
//! `block(height: 575805) { transactions { zkappCommands { zkappCommand { ... } } } }`.
//! It has 11 account updates and no proof.
//!
//! The fee payer's signature is the one on chain. It verifies only if this crate computes the
//! commitment the network computed, which the vectors generated with o1js cannot show.
#![cfg(feature = "mesa-hardfork")]

use ark_ff::PrimeField;
use mina_signer::{BaseField, PubKey, ScalarField, Signature, Signer};
use mina_tx::{
    base58::{from_base58_check, SIGNATURE_VERSION_BYTE},
    network_id::NetworkIdEnvelope,
    zkapp_tx::ZKAppCommand,
    NetworkId, TransactionEnvelope, TransactionKind,
};

const COMMAND: &str = include_str!("data/mainnet/zkapp-575805.json");

fn mainnet_command() -> TransactionEnvelope {
    TransactionEnvelope::from_str_network(COMMAND, NetworkIdEnvelope::from(NetworkId::Mainnet))
        .expect("the Mainnet command parses")
}

fn zkapp(envelope: &TransactionEnvelope) -> &ZKAppCommand {
    match envelope.inner() {
        TransactionKind::ZkApp(command) => command,
        TransactionKind::Legacy(_) => panic!("the Mainnet command is a zkApp command"),
    }
}

/// Decodes a base58 signature: a version number, then the x coordinate of R and s, both
/// little-endian.
fn signature(base58: &str) -> Signature {
    let bytes = from_base58_check(base58, SIGNATURE_VERSION_BYTE).expect("a base58 signature");
    assert_eq!(bytes.len(), 65, "version number, R.x and s");
    Signature::new(
        BaseField::from_le_bytes_mod_order(&bytes[1..33]),
        ScalarField::from_le_bytes_mod_order(&bytes[33..]),
    )
}

fn fee_payer_signature_verifies(envelope: &TransactionEnvelope) -> bool {
    let fee_payer = &zkapp(envelope).fee_payer;
    let key = PubKey::from_address(&fee_payer.body.public_key.0.into_address())
        .expect("the fee payer's public key");
    let mut ctx = mina_signer::create_kimchi(NetworkId::Mainnet);
    ctx.verify(&signature(&fee_payer.authorization), &key, envelope)
}

#[test]
fn mainnet_fee_payer_signature_verifies() {
    assert!(fee_payer_signature_verifies(&mainnet_command()));
}

#[test]
fn mainnet_fee_payer_signature_fails_on_a_changed_amount() {
    let mut envelope = mainnet_command();
    match envelope.inner_mut() {
        TransactionKind::ZkApp(command) => {
            command.account_updates[1].body.balance_change.magnitude += 1
        }
        TransactionKind::Legacy(_) => panic!("the Mainnet command is a zkApp command"),
    }
    assert!(!fee_payer_signature_verifies(&envelope));
}
