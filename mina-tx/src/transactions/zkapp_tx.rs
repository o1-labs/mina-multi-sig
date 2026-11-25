//! Module defining the ZKApp transaction structure, serde, hashing/commitment logic, and related types.
use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use ark_ff::Field as ArkField;
use mina_hasher::{Hashable, ROInput};
use mina_signer::CompressedPubKey;
use serde::{Deserialize, Serialize};

use crate::transactions::network_id::NetworkId;

use crate::transactions::{
    zkapp_tx::{
        commit::zk_commit,
        constants::{APP_STATE_LENGTH, DUMMY_HASH},
    },
    MEMO_BYTES,
};

mod commit;
mod constants;
pub mod packing;
pub mod signature_injection;
pub mod zkapp_display;
pub mod zkapp_graphql;
pub mod zkapp_serde;

// Test vectors module — feature-gated to swap pre-Mesa / Mesa data
#[cfg(any(test, feature = "test-utils"))]
pub mod test_vectors;

/// Compile-time flag indicating whether this build uses Mesa hardfork transaction semantics.
pub const IS_MESA_HARDFORK: bool = cfg!(feature = "mesa-hardfork");

/// Explains the usual reason a zkApp command does not parse: an app state of the other
/// transaction version (8 fields before the Mesa hard fork, 32 from it on).
///
/// `json` is a zkApp command, or the signed output of an earlier session that wraps one.
/// Returns `None` when it holds no app state, or one of the length this build expects.
pub(crate) fn app_state_length_hint(json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let found = value
        .get("accountUpdates")
        .or_else(|| value.pointer("/payload/kind/transaction/accountUpdates"))?
        .as_array()?
        .iter()
        .find_map(|update| update.pointer("/body/update/appState")?.as_array())?
        .len();
    if found == APP_STATE_LENGTH {
        return None;
    }
    Some(alloc::format!(
        "the transaction has {found} appState entries and this build expects {APP_STATE_LENGTH}. \
         A network that runs the Mesa hard fork uses 32 and needs a build with the \
         `mesa-hardfork` feature; an earlier network uses 8 and needs a build without it."
    ))
}

// Re-export signature injection types for convenience
pub use signature_injection::{SignatureInjectionResult, SignatureInjectionWarning};

// -------------------------------------------------------------------------------------------------
// ------------------------------------ Hashing Logic ----------------------------------------------
// -------------------------------------------------------------------------------------------------

// The Hashable representation of a ZKAppCommand for signing purposes
#[derive(Clone, Debug)]
pub struct ZKAppCommandHashable<'a> {
    pub tx: &'a ZKAppCommand,
    pub network: NetworkId,
}

impl<'a> ZKAppCommandHashable<'a> {
    pub fn new(tx: &'a ZKAppCommand, network: NetworkId) -> Self {
        Self { tx, network }
    }
}

impl<'a> Hashable for ZKAppCommandHashable<'a> {
    type D = NetworkId;

    fn domain_string(domain_param: Self::D) -> Option<String> {
        domain_param.into_domain_string().into()
    }

    fn to_roinput(&self) -> mina_hasher::ROInput {
        // Convert the ZKAppCommand into a field element by hashing, return single-field ROInput
        // This code follows O1JS logic, where ZKAppCommand is hashed before being passed to the signature
        let (_, commit) = zk_commit(self.tx, &self.network).unwrap();
        ROInput::new().append_field(commit)
    }
}

// -------------------------------------------------------------------------------------------------
// ----------------------------- ZKApp Transaction Structs -----------------------------------------
// -------------------------------------------------------------------------------------------------

// The final transaction structure for a ZkApp transaction
// FeePayer is a field which may be signed by the same key as in the account updates
// or by a different key
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ZKAppCommand {
    pub fee_payer: FeePayer,
    pub account_updates: Vec<AccountUpdate>,
    #[serde(
        serialize_with = "zkapp_serde::memo_serde",
        deserialize_with = "zkapp_serde::memo_deser"
    )]
    pub memo: [u8; MEMO_BYTES],
}

impl Default for ZKAppCommand {
    fn default() -> Self {
        Self {
            fee_payer: FeePayer::default(),
            account_updates: Vec::default(),
            memo: [0u8; MEMO_BYTES],
        }
    }
}

// Fee payer
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct FeePayer {
    pub body: FeePayerBody,
    pub authorization: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FeePayerBody {
    pub public_key: PublicKey,
    #[serde(
        serialize_with = "serialize_u64_string",
        deserialize_with = "deserialize_u64_string"
    )]
    pub fee: UInt64,
    #[serde(
        serialize_with = "serialize_option_u32_string",
        deserialize_with = "deserialize_option_u32_string"
    )]
    pub valid_until: Option<UInt32>,
    #[serde(
        serialize_with = "serialize_u32_string",
        deserialize_with = "deserialize_u32_string"
    )]
    pub nonce: UInt32,
}

// Account update

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AccountUpdate {
    pub body: AccountUpdateBody,
    pub authorization: Authorization,
}

impl From<FeePayer> for AccountUpdate {
    fn from(fee_payer: FeePayer) -> Self {
        // Unpack fee payer pieces
        let FeePayer {
            body,
            authorization,
        } = fee_payer;
        let public_key = body.public_key;
        let fee_magnitude = body.fee;
        let nonce = body.nonce;
        let vaild_until = body.valid_until.unwrap_or(u32::MAX);

        let account_update = AccountUpdate::default();
        let mut body = account_update.body;

        body.public_key = public_key;
        body.balance_change = BalanceChange {
            magnitude: fee_magnitude,
            sgn: -1,
        };
        body.increment_nonce = true;

        body.preconditions.network.global_slot_since_genesis = {
            Some(RangeCondition {
                lower: StringU32(0),
                upper: StringU32(vaild_until),
            })
        };
        body.preconditions.account.nonce = {
            Some(RangeCondition {
                lower: StringU32(nonce),
                upper: StringU32(nonce),
            })
        };
        body.use_full_commitment = true;
        body.implicit_account_creation_fee = true;
        body.authorization_kind = AuthorizationKind {
            is_proved: false,
            is_signed: true,
            verification_key_hash: *DUMMY_HASH,
        };

        AccountUpdate {
            body,
            authorization: Authorization {
                proof: None,
                signature: Some(authorization),
            },
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AccountUpdateBody {
    pub public_key: PublicKey,
    pub token_id: TokenId,
    pub update: Update,
    pub balance_change: BalanceChange,
    pub increment_nonce: Bool,
    pub events: Events,
    pub actions: Actions,
    pub call_data: Field,
    pub call_depth: u32,
    pub preconditions: Preconditions,
    pub use_full_commitment: Bool,
    pub implicit_account_creation_fee: Bool,
    pub may_use_token: MayUseToken,
    pub authorization_kind: AuthorizationKind,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    pub app_state: [Option<Field>; APP_STATE_LENGTH],
    pub delegate: Option<PublicKey>,
    pub verification_key: Option<VerificationKeyData>,
    pub permissions: Option<Permissions>,
    pub zkapp_uri: Option<ZkappUri>,
    pub token_symbol: Option<TokenSymbol>,
    pub timing: Option<TimingData>,
    pub voting_for: Option<Field>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Permissions {
    pub edit_state: AuthRequired,
    pub access: AuthRequired,
    pub send: AuthRequired,
    pub receive: AuthRequired,
    pub set_delegate: AuthRequired,
    pub set_permissions: AuthRequired,
    pub set_verification_key: SetVerificationKey,
    pub set_zkapp_uri: AuthRequired,
    pub edit_action_state: AuthRequired,
    pub set_token_symbol: AuthRequired,
    pub increment_nonce: AuthRequired,
    pub set_voting_for: AuthRequired,
    pub set_timing: AuthRequired,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SetVerificationKey {
    pub auth: AuthRequired,
    #[serde(
        serialize_with = "serialize_u32_string",
        deserialize_with = "deserialize_u32_string"
    )]
    pub txn_version: UInt32,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Preconditions {
    pub network: NetworkPreconditions,
    pub account: AccountPreconditions,
    pub valid_while: Option<RangeCondition<StringU32>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AccountPreconditions {
    pub balance: Option<RangeCondition<StringU64>>,
    pub nonce: Option<RangeCondition<StringU32>>,
    pub receipt_chain_hash: Option<ReceiptChainHash>,
    pub delegate: Option<PublicKey>,
    pub state: [Option<Field>; APP_STATE_LENGTH],
    pub action_state: Option<ActionState>,
    pub proved_state: Option<Bool>,
    pub is_new: Option<Bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NetworkPreconditions {
    pub snarked_ledger_hash: Option<Field>,
    pub blockchain_length: Option<RangeCondition<StringU32>>,
    pub min_window_density: Option<RangeCondition<StringU32>>,
    pub total_currency: Option<RangeCondition<StringU64>>,
    pub global_slot_since_genesis: Option<RangeCondition<StringU32>>,
    pub staking_epoch_data: EpochData,
    pub next_epoch_data: EpochData,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(transparent)]
pub struct Events {
    pub data: Vec<Vec<Field>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(transparent)]
pub struct Actions {
    pub data: Vec<Vec<Field>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Authorization {
    pub proof: Option<String>,
    pub signature: Option<String>,
}

// Supporting types

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct RangeCondition<T> {
    pub lower: T,
    pub upper: T,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VerificationKeyData {
    pub data: String,
    pub hash: VerificationKeyHash,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TimingData {
    pub initial_minimum_balance: UInt64,
    pub cliff_time: UInt32,
    pub cliff_amount: UInt64,
    pub vesting_period: UInt32,
    pub vesting_increment: UInt64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EpochData {
    pub ledger: EpochLedger,
    pub seed: Option<Field>,
    pub start_checkpoint: Option<Field>,
    pub lock_checkpoint: Option<Field>,
    pub epoch_length: Option<RangeCondition<StringU32>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EpochLedger {
    pub hash: Option<Field>,
    pub total_currency: Option<RangeCondition<StringU64>>,
}

// Wrappers for base types that need additional traits implemented
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicKey(pub CompressedPubKey);

impl Default for PublicKey {
    fn default() -> Self {
        Self(CompressedPubKey::empty())
    }
}

impl From<CompressedPubKey> for PublicKey {
    fn from(key: CompressedPubKey) -> Self {
        Self(key)
    }
}

impl From<PublicKey> for CompressedPubKey {
    fn from(wrapper: PublicKey) -> Self {
        wrapper.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Copy, Default)]
pub struct Field(pub mina_hasher::Fp);

impl From<mina_hasher::Fp> for Field {
    fn from(field: mina_hasher::Fp) -> Self {
        Self(field)
    }
}

impl From<Field> for mina_hasher::Fp {
    fn from(wrapper: Field) -> Self {
        wrapper.0
    }
}

// Base types from transaction-leaves-bigint.ts
pub type Bool = bool;
pub type UInt64 = u64;
pub type UInt32 = u32;
pub type Sign = i8; // -1 or 1

/// A u32 that serializes/deserializes as a string (for JSON compatibility with o1js)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StringU32(pub u32);

impl From<u32> for StringU32 {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<StringU32> for u32 {
    fn from(wrapper: StringU32) -> Self {
        wrapper.0
    }
}

/// A u64 that serializes/deserializes as a string (for JSON compatibility with o1js)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StringU64(pub u64);

impl From<u64> for StringU64 {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<StringU64> for u64 {
    fn from(wrapper: StringU64) -> Self {
        wrapper.0
    }
}

// Wrapper structs
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenId(pub Field);
impl Default for TokenId {
    fn default() -> Self {
        TokenId(Field(mina_hasher::Fp::ONE))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ActionState(pub Field);

// Derived types
pub type StateHash = Field;
pub type VerificationKeyHash = Field;
pub type ReceiptChainHash = Field;
pub type TransactionVersion = UInt32;

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum AuthRequired {
    #[default]
    None,
    Either,
    Proof,
    Signature,
    Impossible,
    Both, // Legacy only
}

pub struct AuthRequiredEncoded<T> {
    pub constant: T,
    pub signature_necessary: T,
    pub signature_sufficient: T,
}

impl AuthRequired {
    pub fn encode(self) -> AuthRequiredEncoded<bool> {
        let (constant, signature_necessary, signature_sufficient) = match self {
            AuthRequired::None => (true, false, true),
            AuthRequired::Either => (false, false, true),
            AuthRequired::Proof => (false, false, false),
            AuthRequired::Signature => (false, true, true),
            AuthRequired::Impossible => (true, true, false),
            AuthRequired::Both => (false, true, false),
        };

        AuthRequiredEncoded {
            constant,
            signature_necessary,
            signature_sufficient,
        }
    }
}

// NOTE: TokenSymbol is serialized by o1js as a JSON string (e.g. "MOCK"), NOT as a byte array.
// derive(Serialize, Deserialize) on Vec<u8> expects a JSON array like [77,79,67,75] which
// doesn't match what o1js produces. Custom serde handles the string <-> bytes conversion.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TokenSymbol(pub Vec<u8>);

impl Serialize for TokenSymbol {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let s = core::str::from_utf8(&self.0).map_err(serde::ser::Error::custom)?;
        serializer.serialize_str(s)
    }
}

impl<'de> Deserialize<'de> for TokenSymbol {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl TokenSymbol {
    pub fn to_bytes(&self, bytes: &mut [u8]) {
        if self.0.is_empty() {
            return;
        }
        let len = self.0.len();
        let s: &[u8] = &self.0;
        bytes[..len].copy_from_slice(&s[..len.min(6)]);
    }
}

impl core::str::FromStr for TokenSymbol {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() <= 6 {
            Ok(Self(s.as_bytes().to_vec()))
        } else {
            Err("Token symbol must be at most 6 characters".to_string())
        }
    }
}

// Default is derived for TokenSymbol

// NOTE: Same issue as TokenSymbol — o1js serializes zkappUri as a plain JSON string
// (e.g. "https://github.com/..."), not a byte array. Custom serde required.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ZkappUri(pub Vec<u8>);

impl Serialize for ZkappUri {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let s = core::str::from_utf8(&self.0).map_err(serde::ser::Error::custom)?;
        serializer.serialize_str(s)
    }
}

impl<'de> Deserialize<'de> for ZkappUri {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl core::str::FromStr for ZkappUri {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // The previous 32-char limit was wrong — o1js doesn't enforce it and real deploy
        // transactions routinely have longer URIs (e.g. GitHub URLs). The packing/hashing
        // logic handles arbitrary-length URIs fine.
        Ok(Self(s.as_bytes().to_vec()))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MayUseToken {
    pub parents_own_token: Bool,
    pub inherit_from_parent: Bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BalanceChange {
    #[serde(
        serialize_with = "serialize_u64_string",
        deserialize_with = "deserialize_u64_string"
    )]
    pub magnitude: UInt64,
    #[serde(
        serialize_with = "serialize_sign",
        deserialize_with = "deserialize_sign"
    )]
    pub sgn: Sign,
}
impl Default for BalanceChange {
    fn default() -> Self {
        Self {
            magnitude: 0,
            sgn: 1,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationKind {
    pub is_signed: Bool,
    pub is_proved: Bool,
    pub verification_key_hash: VerificationKeyHash,
}

impl Default for AuthorizationKind {
    fn default() -> Self {
        Self {
            is_signed: true,
            is_proved: false,
            verification_key_hash: *DUMMY_HASH,
        }
    }
}

// Helper functions for serde
fn serialize_u64_string<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&value.to_string())
}

fn deserialize_u64_string<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.parse().map_err(serde::de::Error::custom)
}

fn serialize_u32_string<S>(value: &u32, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&value.to_string())
}

fn deserialize_u32_string<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.parse().map_err(serde::de::Error::custom)
}

fn serialize_sign<S>(value: &i8, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    if *value == 0 {
        return Err(serde::ser::Error::custom("Sign must be -1 or 1, found 0"));
    }
    let s = if *value > 0 { "Positive" } else { "Negative" };
    serializer.serialize_str(s)
}

fn deserialize_sign<'de, D>(deserializer: D) -> Result<i8, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    match s.as_str() {
        "Positive" => Ok(1),
        "Negative" => Ok(-1),
        _ => Err(serde::de::Error::custom("Invalid sign value")),
    }
}

fn serialize_option_u32_string<S>(value: &Option<u32>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match value {
        Some(v) => serializer.serialize_str(&v.to_string()),
        None => serializer.serialize_none(),
    }
}

fn deserialize_option_u32_string<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(deserializer)?;
    match opt {
        Some(s) => s.parse().map(Some).map_err(serde::de::Error::custom),
        None => Ok(None),
    }
}
