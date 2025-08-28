use chrono::{DateTime, Utc};
// Use miden-objects
pub use miden_objects::{
    Felt,
    account::AccountId,
    block::BlockNumber,
    note::{Note, NoteDetails, NoteHeader, NoteId, NoteInclusionProof, NoteTag, NoteType},
    utils::Serializable,
};
use miden_private_transport_proto::miden_private_transport::encryption_key;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NoteStatus {
    Sent,
    Duplicate,
}

/// Types of encryption keys supported
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EncryptionKeyType {
    Aes256Gcm,
    X25519Pub,
    Other,
}

/// A note stored in the database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredNote {
    #[serde(
        serialize_with = "serialize_note_header",
        deserialize_with = "deserialize_note_header"
    )]
    pub header: NoteHeader,
    pub encrypted_data: Vec<u8>,
    pub created_at: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    pub received_by: Option<Vec<String>>,
}

/// Information about a note in API responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteInfo {
    #[serde(
        serialize_with = "serialize_note_header",
        deserialize_with = "deserialize_note_header"
    )]
    pub header: NoteHeader,
    pub encrypted_data: Vec<u8>,
    pub created_at: DateTime<Utc>,
}

/// An encryption key stored in the database
#[derive(Debug, Clone)]
pub struct StoredEncryptionKey {
    pub account_id: AccountId,
    pub key_type: EncryptionKeyType,
    pub key_data: Vec<u8>,
    pub created_at: DateTime<Utc>,
}

impl StoredEncryptionKey {
    /// Serialize the account ID to bytes for storage
    pub fn account_id_bytes(&self) -> Vec<u8> {
        self.account_id.to_bytes()
    }

    /// Create from stored data
    pub fn from_stored(
        account_id: AccountId,
        key_type: EncryptionKeyType,
        key_data: Vec<u8>,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            account_id,
            key_type,
            key_data,
            created_at,
        }
    }
}

impl From<&encryption_key::Value> for EncryptionKeyType {
    fn from(value: &encryption_key::Value) -> Self {
        match value {
            encryption_key::Value::Aes256gcm(_) => EncryptionKeyType::Aes256Gcm,
            encryption_key::Value::X25519Pub(_) => EncryptionKeyType::X25519Pub,
            encryption_key::Value::Other(_) => EncryptionKeyType::Other,
        }
    }
}

impl From<EncryptionKeyType> for encryption_key::Value {
    fn from(key_type: EncryptionKeyType) -> Self {
        match key_type {
            EncryptionKeyType::Aes256Gcm => encryption_key::Value::Aes256gcm(vec![]),
            EncryptionKeyType::X25519Pub => encryption_key::Value::X25519Pub(vec![]),
            EncryptionKeyType::Other => encryption_key::Value::Other(vec![]),
        }
    }
}

fn serialize_note_header<S>(note_header: &NoteHeader, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use miden_objects::utils::Serializable;
    serializer.serialize_bytes(&note_header.to_bytes())
}

fn deserialize_note_header<'de, D>(deserializer: D) -> Result<NoteHeader, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use miden_objects::utils::Deserializable;
    use serde::de::Error;
    let bytes = Vec::<u8>::deserialize(deserializer)?;
    NoteHeader::read_from_bytes(&bytes).map_err(|e| {
        D::Error::custom(format!("Failed to deserialize NoteHeader from bytes: {e:?}"))
    })
}

pub fn random_note_id() -> NoteId {
    use miden_objects::{Digest, Felt, Word};
    use rand::Rng;

    let mut rng = rand::rng();

    let recipient_word = Word::from([
        Felt::new(rng.random::<u64>()),
        Felt::new(rng.random::<u64>()),
        Felt::new(rng.random::<u64>()),
        Felt::new(rng.random::<u64>()),
    ]);
    let asset_commitment_word = Word::from([
        Felt::new(rng.random::<u64>()),
        Felt::new(rng.random::<u64>()),
        Felt::new(rng.random::<u64>()),
        Felt::new(rng.random::<u64>()),
    ]);

    let recipient = Digest::from(recipient_word);
    let asset_commitment = Digest::from(asset_commitment_word);

    NoteId::new(recipient, asset_commitment)
}

pub const TEST_TAG: u32 = 3_221_225_472;
pub fn test_note_header() -> NoteHeader {
    use miden_objects::{
        Felt,
        account::AccountId,
        note::{NoteExecutionHint, NoteMetadata, NoteType},
        testing::account_id::ACCOUNT_ID_MAX_ZEROES,
    };

    let id = random_note_id();
    let sender = AccountId::try_from(ACCOUNT_ID_MAX_ZEROES).unwrap();
    let note_type = NoteType::Private;
    let tag = NoteTag::from_account_id(sender);
    let aux = Felt::try_from(0xffff_ffff_0000_0000u64).unwrap();
    let execution_hint = NoteExecutionHint::None;

    let metadata = NoteMetadata::new(sender, note_type, tag, execution_hint, aux).unwrap();

    NoteHeader::new(id, metadata)
}
