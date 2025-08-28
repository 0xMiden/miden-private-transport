use std::{collections::HashMap, time::Duration};

use chrono::{DateTime, Utc};
use miden_objects::utils::{Deserializable, Serializable};
use miden_private_transport_proto::{
    AccountId as ProtoAccountId,
    miden_private_transport::{
        EncryptedNote, EncryptionKey as ProtoEncryptionKey, FetchKeyRequest, FetchNotesRequest,
        RegisterKeyRequest, SendNoteRequest, encryption_key,
        miden_private_transport_client::MidenPrivateTransportClient,
    },
};
use prost_types;
use tonic::{
    Request,
    transport::{Channel, ClientTlsConfig},
};
use tower::timeout::Timeout;

use crate::{
    Error, Result, SerializableKey,
    types::{AccountId, NoteHeader, NoteId, NoteInfo, NoteTag},
};

#[derive(Clone)]
pub struct GrpcClient {
    client: MidenPrivateTransportClient<Timeout<Channel>>,
    // Last fetched timestamp
    lts: HashMap<NoteTag, DateTime<Utc>>,
}

impl GrpcClient {
    pub async fn connect(endpoint: String, timeout_ms: u64) -> Result<Self> {
        let tls = ClientTlsConfig::new().with_native_roots();
        let channel = Channel::from_shared(endpoint.clone())
            .map_err(|e| Error::Internal(format!("Invalid endpoint URI: {e}")))?
            .tls_config(tls)?
            .connect()
            .await?;
        let timeout = Duration::from_millis(timeout_ms);
        let timeout_channel = Timeout::new(channel, timeout);
        let client = MidenPrivateTransportClient::new(timeout_channel);
        let lts = HashMap::new();

        Ok(Self { client, lts })
    }

    pub async fn send_note(
        &mut self,
        header: NoteHeader,
        encrypted_details: Vec<u8>,
    ) -> Result<NoteId> {
        let request = SendNoteRequest {
            note: Some(EncryptedNote {
                header: header.to_bytes(),
                encrypted_details,
            }),
        };

        let response = self
            .client
            .clone()
            .send_note(Request::new(request))
            .await
            .map_err(|e| Error::Internal(format!("Send note failed: {e:?}")))?;

        let response = response.into_inner();

        // Parse note ID from hex string
        let note_id = NoteId::try_from_hex(&response.id)
            .map_err(|e| Error::Internal(format!("Invalid note ID: {e:?}")))?;

        Ok(note_id)
    }

    pub async fn fetch_notes(&mut self, tag: NoteTag) -> Result<Vec<NoteInfo>> {
        let ts = self.lts.get(&tag).copied().unwrap_or(DateTime::from_timestamp(0, 0).unwrap());
        let request = FetchNotesRequest {
            tag: tag.as_u32(),
            timestamp: Some(prost_types::Timestamp {
                seconds: ts.timestamp(),
                nanos: ts
                    .timestamp_subsec_nanos()
                    .try_into()
                    .map_err(|_| Error::Internal("Timestamp nanoseconds too large".to_string()))?,
            }),
        };

        let response = self
            .client
            .clone()
            .fetch_notes(Request::new(request))
            .await
            .map_err(|e| Error::Internal(format!("Fetch notes failed: {e:?}")))?;

        let response = response.into_inner();

        // Convert protobuf notes to internal format and track the most recent received timestamp
        let mut notes = Vec::new();
        let mut latest_received_at = ts;

        for note in response.notes {
            let header = NoteHeader::read_from_bytes(&note.header)
                .map_err(|e| Error::Internal(format!("Invalid note header: {e:?}")))?;

            // Convert protobuf timestamp to DateTime
            let received_at = if let Some(timestamp) = note.timestamp {
                chrono::DateTime::from_timestamp(
                    timestamp.seconds,
                    timestamp.nanos.try_into().map_err(|_| {
                        Error::Internal("Negative timestamp nanoseconds".to_string())
                    })?,
                )
                .ok_or_else(|| Error::Internal("Invalid timestamp".to_string()))?
            } else {
                Utc::now() // Fallback to current time if timestamp is missing
            };

            // Update the latest received timestamp
            if received_at > latest_received_at {
                latest_received_at = received_at;
            }

            notes.push(NoteInfo {
                header,
                encrypted_data: note.encrypted_details,
                created_at: received_at,
            });
        }

        // Update the last timestamp to the most recent received timestamp
        self.lts.insert(tag, latest_received_at);

        Ok(notes)
    }

    pub async fn register_key(
        &mut self,
        account_id: AccountId,
        key: SerializableKey,
    ) -> Result<()> {
        // Convert to proto types
        let value = match key {
            SerializableKey::Aes256Gcm(data) => {
                encryption_key::Value::Aes256gcm(data.as_bytes().to_vec())
            },
            SerializableKey::X25519Pub(data) => {
                encryption_key::Value::X25519Pub(data.as_bytes().to_vec())
            },
            SerializableKey::X25519(_) => {
                return Err(Error::Internal(
                    "Attempting to register a key pair or private key".to_string(),
                ));
            },
        };
        let proto_encryption_key = ProtoEncryptionKey { value: Some(value) };
        let request = RegisterKeyRequest {
            account_id: Some(ProtoAccountId { id: account_id.to_bytes() }),
            encryption_key: Some(proto_encryption_key),
        };

        // Push key
        self.client.register_key(Request::new(request)).await?;

        Ok(())
    }

    pub async fn fetch_key(&mut self, account_id: AccountId) -> Result<Option<SerializableKey>> {
        let request = FetchKeyRequest {
            account_id: Some(ProtoAccountId { id: account_id.to_bytes() }),
        };

        // Fetch key
        let key = self.client.fetch_key(request).await?.into_inner().encryption_key;

        // Convert from proto types
        let value = key.and_then(|val| val.value);
        let serkey = if let Some(key) = value {
            match key {
                encryption_key::Value::Aes256gcm(data) => {
                    let key_array: [u8; 32] = data
                        .try_into()
                        .map_err(|_| Error::Internal("Invalid AES key length".to_string()))?;
                    let aes_key = crate::crypto::aes::Aes256GcmKey::new(key_array);
                    Some(SerializableKey::Aes256Gcm(aes_key))
                },
                encryption_key::Value::X25519Pub(data) => {
                    let key_array: [u8; 32] = data
                        .try_into()
                        .map_err(|_| Error::Internal("Invalid X25519 key length".to_string()))?;
                    let pubkey = crate::crypto::hybrid::X25519PublicKey::from(key_array);
                    Some(SerializableKey::X25519Pub(pubkey))
                },
                encryption_key::Value::Other(_) => None,
            }
        } else {
            None
        };

        Ok(serkey)
    }
}

#[async_trait::async_trait]
impl super::TransportClient for GrpcClient {
    async fn send_note(
        &mut self,
        header: NoteHeader,
        encrypted_note: Vec<u8>,
    ) -> Result<(NoteId, crate::types::NoteStatus)> {
        let note_id = self.send_note(header, encrypted_note).await?;
        Ok((note_id, crate::types::NoteStatus::Sent))
    }

    async fn fetch_notes(&mut self, tag: NoteTag) -> Result<Vec<crate::types::NoteInfo>> {
        self.fetch_notes(tag).await
    }

    async fn register_key(&mut self, account_id: AccountId, key: SerializableKey) -> Result<()> {
        self.register_key(account_id, key).await
    }

    async fn fetch_key(&mut self, account_id: AccountId) -> Result<Option<SerializableKey>> {
        self.fetch_key(account_id).await
    }
}
