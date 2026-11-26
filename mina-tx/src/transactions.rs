//! This file defines the generic TransactionEnvelope structure that encapsulates all kinds of transactions that Mina supports. It is the structure that we sign using FROST.

use crate::{
    errors::MinaTxError,
    signatures::{Sig, TransactionSignature},
    transactions::{
        legacy_tx::LegacyTransaction,
        network_id::NetworkIdEnvelope,
        zkapp_tx::{
            zkapp_display::json_display, zkapp_graphql, ZKAppCommand, ZKAppCommandHashable,
        },
    },
};
use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use mina_hasher::Hashable;
use serde::{Deserialize, Serialize};

use crate::transactions::network_id::NetworkId;

pub mod legacy_tx;
pub mod network_id;
pub mod zkapp_tx;

pub const MEMO_BYTES: usize = 34;
const MEMO_HEADER_BYTES: usize = 2; // 0x01 + length byte

// Enum distinguishing between legacy and zkApp transactions
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "tag", content = "transaction")]
pub enum TransactionKind {
    ZkApp(ZKAppCommand),
    Legacy(LegacyTransaction),
}

impl TransactionKind {
    pub fn new_zkapp(tx: ZKAppCommand) -> Self {
        TransactionKind::ZkApp(tx)
    }

    pub fn new_legacy(tx: LegacyTransaction) -> Self {
        TransactionKind::Legacy(tx)
    }
    pub fn is_legacy(&self) -> bool {
        match self {
            TransactionKind::Legacy(_) => true,
            TransactionKind::ZkApp(_) => false,
        }
    }
}

// The TransactionEnvelope encapsulates either a legacy transaction or a zkApp transaction along with the network ID.
// Should be the only structure necessary to access when signing transactions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TransactionEnvelope {
    network_id: NetworkIdEnvelope,
    kind: TransactionKind,
}

impl core::fmt::Display for TransactionEnvelope {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        json_display(self, f)
    }
}

impl TransactionEnvelope {
    pub fn new(network_id: NetworkId, kind: TransactionKind) -> Self {
        Self {
            network_id: network_id.into(),
            kind,
        }
    }

    pub fn new_zkapp(network_id: NetworkId, tx: ZKAppCommand) -> Self {
        Self::new(network_id, TransactionKind::new_zkapp(tx))
    }

    pub fn new_legacy(network_id: NetworkId, tx: LegacyTransaction) -> Self {
        Self::new(network_id, TransactionKind::new_legacy(tx))
    }

    /// Parse a Legacy or ZkApp transaction from a JSON string.
    /// Auto-detects the transaction type by attempting to parse as each type.
    /// Tries ZkApp first, then Legacy.
    /// Returns an error if parsing fails for both types.
    pub fn from_str_network(s: &str, network_id: NetworkIdEnvelope) -> Result<Self, MinaTxError> {
        let s = s.trim();

        // Try parsing as a TransactionSignature first (output from a previous signing session).
        // This enables chained multi-group signing where the output of one session is fed as
        // input to the next. The inner payload already has the previous signature injected.
        if let Ok(signed) = serde_json::from_str::<TransactionSignature>(s) {
            return Ok(signed.payload);
        }

        // Try parsing as ZkApp transaction first, then Legacy.
        // IMPORTANT: Do NOT silently swallow parse errors here. If both fail, the caller
        // needs to see the actual serde errors to diagnose the problem — not a generic
        // "unknown transaction type" message that tells them nothing.
        let zkapp_err = match serde_json::from_str::<ZKAppCommand>(s) {
            Ok(zkapp) => return Ok(Self::new_zkapp(network_id.0, zkapp)),
            Err(e) => e,
        };

        let legacy_err = match serde_json::from_str::<LegacyTransaction>(s) {
            Ok(legacy) => return Ok(Self::new_legacy(network_id.0, legacy)),
            Err(e) => e,
        };

        // serde only reports where the app state stops matching ("trailing characters"), so
        // say it when the command was made for the other transaction version.
        let hint = zkapp_tx::app_state_length_hint(s)
            .map(|hint| alloc::format!("\n  Hint: {hint}"))
            .unwrap_or_default();

        Err(MinaTxError::UnknownTransactionType(
            alloc::format!(
                "Unable to parse transaction as ZkApp or Legacy.\n  ZkApp parse error: {}\n  Legacy parse error: {}{}",
                zkapp_err,
                legacy_err,
                hint
            )
        ))
    }

    pub fn inner(&self) -> &TransactionKind {
        &self.kind
    }

    /// Get a mutable reference to the inner transaction kind.
    pub fn inner_mut(&mut self) -> &mut TransactionKind {
        &mut self.kind
    }

    /// Serialize the TransactionEnvelope to a byte vector using serde.
    pub fn serialize(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// Deserialize a TransactionEnvelope from a byte slice using serde.
    pub fn deserialize(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }

    pub fn network_id(&self) -> NetworkId {
        self.network_id.0.clone()
    }

    pub fn is_legacy(&self) -> bool {
        self.kind.is_legacy()
    }

    pub fn to_graphql_query_json(&self, signature: Sig) -> Result<String, serde_json::Error> {
        match &self.kind {
            TransactionKind::ZkApp(zkapp) => {
                let mutation = zkapp_graphql::build_send_zkapp_mutation(zkapp);
                serde_json::to_string_pretty(&mutation)
            }
            TransactionKind::Legacy(legacy_tx) => {
                let sig_input = crate::graphql::SignatureInput::FieldScalar {
                    field: signature.field.to_string(),
                    scalar: signature.scalar.to_string(),
                };

                if legacy_tx.is_delegation() {
                    let input = crate::graphql::SendDelegationInput::from(legacy_tx);
                    let mutation = crate::graphql::build_send_delegation_mutation(input, sig_input);
                    serde_json::to_string_pretty(&mutation)
                } else {
                    let input = crate::graphql::SendPaymentInput::from(legacy_tx);
                    let mutation = crate::graphql::build_send_payment_mutation(input, sig_input);
                    serde_json::to_string_pretty(&mutation)
                }
            }
        }
    }
}

impl Hashable for TransactionEnvelope {
    type D = NetworkId;

    fn domain_string(domain_param: Self::D) -> Option<String> {
        domain_param.into_domain_string().into()
    }

    fn to_roinput(&self) -> mina_hasher::ROInput {
        match &self.kind {
            TransactionKind::ZkApp(zkapp_tx) => {
                let zkapp_hashable = ZKAppCommandHashable::new(zkapp_tx, self.network_id.0.clone());
                zkapp_hashable.to_roinput()
            }
            TransactionKind::Legacy(legacy_tx) => legacy_tx.to_roinput(),
        }
    }
}

#[cfg(test)]
mod tests {
    use mina_signer::Keypair;

    use crate::{
        errors::MinaTxError, transactions::zkapp_tx::test_vectors::get_zkapp_test_vectors,
    };

    use super::*;

    #[test]
    fn test_transaction_envelope_serialization_roundtrip() {
        let private_key_hex = "35dcca7620128d240cc3319c83dc6402ad439038361ba853af538a4cea3ddabc";
        let mina_keypair = Keypair::from_hex(private_key_hex)
            .map_err(|_| MinaTxError::InvalidSignature("Failed to parse keypair".into()))
            .unwrap();

        let legacy_tx = LegacyTransaction::new_payment(
            mina_keypair.public.clone(),
            mina_keypair.public.clone(),
            1000,
            1,
            0,
        );

        let envelope = TransactionEnvelope::new_legacy(NetworkId::Testnet, legacy_tx);

        let serialized = envelope.serialize().expect("Serialization failed");
        let deserialized =
            TransactionEnvelope::deserialize(&serialized).expect("Deserialization failed");

        assert_eq!(deserialized, envelope);
    }

    #[test]
    fn test_transaction_envelope_zkapp_roundtrip() {
        // Iterate through each test vector in test_vectors
        let test_vectors = get_zkapp_test_vectors();

        for tv in test_vectors {
            let network_id = tv.network;
            let zkapp_tx = tv.zkapp_command;

            let envelope = TransactionEnvelope::new_zkapp(network_id, zkapp_tx.clone());

            let serialized = envelope.serialize().expect("Serialization failed");
            let deserialized =
                TransactionEnvelope::deserialize(&serialized).expect("Deserialization failed");

            assert_eq!(
                deserialized, envelope,
                "ZkApp TransactionEnvelope roundtrip failed for test vector {}",
                tv.name
            );
        }
    }

    #[test]
    fn test_from_str_network_legacy_payment() {
        let json = r#"{
            "to": "B62qiy32p8kAKnny8ZFwoMhYpBppM1DWVCqAPBYNcXnsAHhnfAAuXgg",
            "from": "B62qiy32p8kAKnny8ZFwoMhYpBppM1DWVCqAPBYNcXnsAHhnfAAuXgg",
            "fee": "10000",
            "amount": "1000000",
            "nonce": "42",
            "memo": "test",
            "valid_until": "12345",
            "tag": [false, false, false]
        }"#;

        let result = TransactionEnvelope::from_str_network(
            json,
            NetworkIdEnvelope::from(NetworkId::Testnet),
        );
        assert!(result.is_ok());
        let envelope = result.unwrap();
        assert!(envelope.is_legacy());
        assert_eq!(envelope.network_id(), NetworkId::Testnet);
    }

    #[cfg(not(feature = "mesa-hardfork"))]
    #[test]
    fn test_from_str_network_zkapp() {
        let json = include_str!("../tests/data/payment-zkapp.json");
        let result = TransactionEnvelope::from_str_network(
            json,
            NetworkIdEnvelope::from(NetworkId::Mainnet),
        );
        assert!(result.is_ok());
        let envelope = result.unwrap();
        assert!(!envelope.is_legacy());
        assert_eq!(envelope.network_id(), NetworkId::Mainnet);
    }

    /// Regression test: deploy-v0.0.4-unsigned.json fails to parse because ZkappUri and
    /// TokenSymbol use derive(Serialize, Deserialize) on Vec<u8>, which expects a JSON
    /// array of integers. But o1js serializes these as plain strings.
    /// e.g. "zkappUri": "https://..." and "tokenSymbol": "MOCKnE"
    #[cfg(not(feature = "mesa-hardfork"))]
    #[test]
    fn test_parse_deploy_v004_zkapp_uri_as_string() {
        let json = include_str!("../tests/data/deploy-v0.0.4-unsigned.json");
        let result = TransactionEnvelope::from_str_network(
            json,
            NetworkIdEnvelope::from(NetworkId::Testnet),
        );
        assert!(
            result.is_ok(),
            "Failed to parse deploy-v0.0.4-unsigned.json: {:?}",
            result.unwrap_err()
        );
        let envelope = result.unwrap();
        assert!(!envelope.is_legacy());
    }

    /// Minimal reproduction: zkappUri as a string should parse, not require a byte array
    #[cfg(not(feature = "mesa-hardfork"))]
    #[test]
    fn test_zkapp_uri_string_field() {
        let json = include_str!("../tests/data/deploy-contract.json");
        // First ensure the base parses fine (zkappUri: null)
        let base_result = serde_json::from_str::<ZKAppCommand>(json);
        assert!(
            base_result.is_ok(),
            "Base deploy-contract.json should parse"
        );

        // Now inject a string zkappUri value like o1js produces
        let modified = json.replace(
            "\"zkappUri\": null",
            "\"zkappUri\": \"https://example.com\"",
        );
        let result = serde_json::from_str::<ZKAppCommand>(&modified);
        assert!(
            result.is_ok(),
            "ZkApp with string zkappUri should parse but got: {}",
            result.unwrap_err()
        );
    }

    /// ZkappUri with more than 32 characters should parse — the 32-char limit is wrong,
    /// o1js and the Mina protocol don't enforce it.
    #[cfg(not(feature = "mesa-hardfork"))]
    #[test]
    fn test_zkapp_uri_longer_than_32_chars() {
        let json = include_str!("../tests/data/deploy-contract.json");
        let long_uri = "https://github.com/nori-zk/mock-nori-bridge"; // 45 chars
        assert!(long_uri.len() > 32);

        let modified = json.replace(
            "\"zkappUri\": null",
            &alloc::format!("\"zkappUri\": \"{}\"", long_uri),
        );
        let result = serde_json::from_str::<ZKAppCommand>(&modified);
        assert!(
            result.is_ok(),
            "ZkApp with >32 char zkappUri should parse but got: {}",
            result.unwrap_err()
        );
    }

    /// Minimal reproduction: tokenSymbol as a string should parse, not require a byte array
    #[cfg(not(feature = "mesa-hardfork"))]
    #[test]
    fn test_token_symbol_string_field() {
        let json = include_str!("../tests/data/deploy-contract.json");

        // Inject a string tokenSymbol value like o1js produces
        let modified = json.replace("\"tokenSymbol\": null", "\"tokenSymbol\": \"MOCK\"");
        let result = serde_json::from_str::<ZKAppCommand>(&modified);
        assert!(
            result.is_ok(),
            "ZkApp with string tokenSymbol should parse but got: {}",
            result.unwrap_err()
        );
    }

    /// A TransactionSignature (output from a previous FROST signing session) should be
    /// parseable as input for a subsequent signing session, enabling chained multi-group
    /// signing of the same transaction.
    #[cfg(not(feature = "mesa-hardfork"))]
    #[test]
    fn test_parse_signed_transaction_as_input() {
        let json = include_str!("../tests/data/deploy-v0.0.6-admin-signed.json");
        let result = TransactionEnvelope::from_str_network(
            json,
            NetworkIdEnvelope::from(NetworkId::Testnet),
        );
        assert!(
            result.is_ok(),
            "Signed transaction should be parseable as input: {:?}",
            result.unwrap_err()
        );
        let envelope = result.unwrap();
        assert!(!envelope.is_legacy());
    }

    #[test]
    fn test_from_str_network_invalid_json() {
        let result = TransactionEnvelope::from_str_network(
            "not json",
            NetworkIdEnvelope::from(NetworkId::Testnet),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_from_str_network_unrecognized_type() {
        let json = r#"{"unknown": "field"}"#;
        let result = TransactionEnvelope::from_str_network(
            json,
            NetworkIdEnvelope::from(NetworkId::Testnet),
        );
        assert!(result.is_err());
    }

    /// A zkApp command made for the other transaction version does not parse. The error must
    /// say why, not only where serde stopped.
    #[test]
    fn test_from_str_network_names_the_app_state_length() {
        // Commands from before Mesa for a Mesa build, commands made after Mesa otherwise:
        // a bare one, then the signed output of an earlier session.
        #[cfg(feature = "mesa-hardfork")]
        let (commands, found) = (
            [
                include_str!("../tests/data/payment-zkapp.json"),
                include_str!("../tests/data/graphql/zkapp_signed.json"),
            ],
            8,
        );
        #[cfg(not(feature = "mesa-hardfork"))]
        let (commands, found) = (
            [
                include_str!("../tests/data/mainnet/zkapp-575805.json"),
                include_str!("../tests/data/graphql/zkapp_signed_mesa.json"),
            ],
            32,
        );

        for json in commands {
            let error = TransactionEnvelope::from_str_network(
                json,
                NetworkIdEnvelope::from(NetworkId::Mainnet),
            )
            .expect_err("a command of the other transaction version must not parse");
            let message = error.to_string();

            assert!(
                message.contains(&alloc::format!(
                    "the transaction has {found} appState entries"
                )),
                "{message}"
            );
            assert!(message.contains("mesa-hardfork"), "{message}");
        }
    }

    #[test]
    fn test_from_str_network_gives_no_hint_without_an_app_state() {
        let error = TransactionEnvelope::from_str_network(
            r#"{"unknown": "field"}"#,
            NetworkIdEnvelope::from(NetworkId::Testnet),
        )
        .expect_err("not a transaction");
        assert!(!error.to_string().contains("Hint"), "{error}");
    }
}
