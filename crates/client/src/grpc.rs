use std::{collections::HashMap, time::Duration};

use chrono::{DateTime, Utc};
use miden_objects::utils::{Deserializable, Serializable};
use miden_private_transport_proto::miden_private_transport::{
    FetchNotesRequest, SendNoteRequest, TransportNote,
    miden_private_transport_client::MidenPrivateTransportClient,
};
use prost_types;
use tonic::{
    Request,
    transport::{Channel, ClientTlsConfig},
};
use tower::timeout::Timeout;

use crate::{
    Error, Result,
    types::{NoteHeader, NoteId, NoteInfo, NoteTag},
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

    pub async fn send_note(&mut self, header: NoteHeader, details: Vec<u8>) -> Result<NoteId> {
        let request = SendNoteRequest {
            note: Some(TransportNote { header: header.to_bytes(), details }),
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

        for pts_note in response.notes {
            let note = pts_note
                .note
                .ok_or_else(|| Error::Internal("Fetched note has no data".to_string()))?;
            let header = NoteHeader::read_from_bytes(&note.header)
                .map_err(|e| Error::Internal(format!("Invalid note header: {e:?}")))?;

            // Convert protobuf timestamp to DateTime
            let received_at = if let Some(timestamp) = pts_note.timestamp {
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
                details: note.details,
                created_at: received_at,
            });
        }

        // Update the last timestamp to the most recent received timestamp
        self.lts.insert(tag, latest_received_at);

        Ok(notes)
    }
}

#[async_trait::async_trait]
impl super::TransportClient for GrpcClient {
    async fn send_note(
        &mut self,
        header: NoteHeader,
        details: Vec<u8>,
    ) -> Result<(NoteId, crate::types::NoteStatus)> {
        let note_id = self.send_note(header, details).await?;
        Ok((note_id, crate::types::NoteStatus::Sent))
    }

    async fn fetch_notes(&mut self, tag: NoteTag) -> Result<Vec<crate::types::NoteInfo>> {
        self.fetch_notes(tag).await
    }
}
