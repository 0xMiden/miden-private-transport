use chrono::{DateTime, Utc};
use miden_lib::{account::wallets::BasicWallet, note::create_p2id_note};
// Use miden-objects
pub use miden_objects::{
    Felt,
    account::AccountId,
    block::BlockNumber,
    note::{Note, NoteDetails, NoteHeader, NoteId, NoteInclusionProof, NoteTag, NoteType},
};
use miden_objects::{
    account::{AccountBuilder, AccountStorageMode},
    crypto::rand::RpoRandomCoin,
};
use miden_testing::Auth;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NoteStatus {
    Sent,
    Duplicate,
}

/// A note stored in the database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredNote {
    #[serde(
        serialize_with = "serialize_note_header",
        deserialize_with = "deserialize_note_header"
    )]
    pub header: NoteHeader,
    pub details: Vec<u8>,
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
    pub details: Vec<u8>,
    pub created_at: DateTime<Utc>,
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

pub fn mock_note_p2id() -> miden_objects::note::Note {
    use rand::Rng;
    let mut rng = rand::rng();
    let (sender, _seed) = AccountBuilder::new(rng.random())
        .storage_mode(AccountStorageMode::Private)
        .with_component(BasicWallet)
        .with_auth_component(Auth::BasicAuth)
        .build()
        .unwrap();
    let (target, _seed) = AccountBuilder::new(rng.random())
        .storage_mode(AccountStorageMode::Private)
        .with_component(BasicWallet)
        .with_auth_component(Auth::BasicAuth)
        .build()
        .unwrap();
    let mut rng = RpoRandomCoin::new(Default::default());
    create_p2id_note(sender.id(), target.id(), vec![], NoteType::Private, Felt::default(), &mut rng)
        .unwrap()
}

/// Create a mock P2ID note with specified sender and target account IDs
pub fn mock_note_p2id_with_accounts(
    sender_id: miden_objects::account::AccountId,
    target_id: miden_objects::account::AccountId,
) -> miden_objects::note::Note {
    let mut rng = RpoRandomCoin::new(Default::default());
    create_p2id_note(sender_id, target_id, vec![], NoteType::Private, Felt::default(), &mut rng)
        .unwrap()
}

pub fn mock_note_p2id_with_tag_and_accounts(
    tag: NoteTag,
    sender: AccountId,
    target: AccountId,
) -> miden_objects::note::Note {
    use miden_objects::{
        Felt,
        asset::{Asset, FungibleAsset},
        crypto::rand::FeltRng,
        note::{NoteAssets, NoteExecutionHint, NoteMetadata},
        testing::account_id::ACCOUNT_ID_PRIVATE_FUNGIBLE_FAUCET,
    };
    use rand::{Rng, RngCore};

    let mut randrng = rand::rng();
    let seed: [Felt; 4] = std::array::from_fn(|_| Felt::new(randrng.next_u64()));
    let mut rng = RpoRandomCoin::new(seed);
    let serial_num = rng.draw_word();
    let recipient = miden_lib::note::utils::build_p2id_recipient(target, serial_num).unwrap();

    let metadata = NoteMetadata::new(
        sender,
        NoteType::Private,
        tag,
        NoteExecutionHint::always(),
        Felt::default(),
    )
    .unwrap();

    let faucet_id = AccountId::try_from(ACCOUNT_ID_PRIVATE_FUNGIBLE_FAUCET).unwrap();
    let asset = Asset::Fungible(FungibleAsset::new(faucet_id, rng.random_range(..50000)).unwrap());
    let vault = NoteAssets::new(vec![asset]).unwrap();

    Note::new(vault, metadata, recipient)
}

/// Create a mock account ID for testing purposes
pub fn mock_account_id() -> miden_objects::account::AccountId {
    use rand::Rng;
    let mut rng = rand::rng();
    let (account, _seed) = AccountBuilder::new(rng.random())
        .storage_mode(AccountStorageMode::Private)
        .with_component(BasicWallet)
        .with_auth_component(Auth::BasicAuth)
        .build()
        .unwrap();
    account.id()
}
