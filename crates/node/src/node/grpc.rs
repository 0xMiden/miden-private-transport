use std::{net::SocketAddr, sync::Arc};

use chrono::{DateTime, Utc};
use miden_objects::utils::{Deserializable, Serializable};
use miden_private_transport_proto::miden_private_transport::{
    FetchNotesRequest, FetchNotesResponse, HealthResponse, SendNoteRequest, SendNoteResponse,
    StatsResponse, miden_private_transport_server::MidenPrivateTransportServer,
};

use crate::{database::Database, metrics::MetricsGrpc};

pub struct GrpcServer {
    database: Arc<Database>,
    config: GrpcServerConfig,
    metrics: MetricsGrpc,
}

#[derive(Clone, Debug)]
pub struct GrpcServerConfig {
    pub host: String,
    pub port: u16,
    pub max_note_size: usize,
}

impl Default for GrpcServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
            max_note_size: 1024 * 1024,
        }
    }
}

impl GrpcServer {
    pub fn new(database: Arc<Database>, config: GrpcServerConfig, metrics: MetricsGrpc) -> Self {
        Self { database, config, metrics }
    }

    pub fn into_service(self) -> MidenPrivateTransportServer<Self> {
        MidenPrivateTransportServer::new(self)
    }

    pub async fn serve(self) -> crate::Result<()> {
        let addr = format!("{}:{}", self.config.host, self.config.port)
            .parse::<SocketAddr>()
            .map_err(|e| crate::Error::Internal(format!("Invalid address: {e}")))?;

        tonic::transport::Server::builder()
            .add_service(self.into_service())
            .serve(addr)
            .await
            .map_err(|e| crate::Error::Internal(format!("Server error: {e}")))
    }
}

#[tonic::async_trait]
impl miden_private_transport_proto::miden_private_transport::miden_private_transport_server::MidenPrivateTransport
    for GrpcServer
{
    #[tracing::instrument(skip(self), fields(operation = "grpc.send_note.request"))]
    async fn send_note(
        &self,
        request: tonic::Request<SendNoteRequest>,
    ) -> Result<tonic::Response<SendNoteResponse>, tonic::Status> {
        let request_data = request.into_inner();
        let note = request_data.note.ok_or_else(|| tonic::Status::invalid_argument("Missing note"))?;

        let timer = self.metrics.grpc_send_note_request((note.header.len() + note.encrypted_details.len()) as u64);

        // Validate note size
        if note.encrypted_details.len() > self.config.max_note_size {
            return Err(tonic::Status::resource_exhausted(format!("Note too large ({})", note.encrypted_details.len())));
        }

        // Convert protobuf request to internal types
        let header = miden_objects::note::NoteHeader::read_from_bytes(&note.header)
            .map_err(|e| {
                tonic::Status::invalid_argument(format!("Invalid header: {e:?}"))
            })?;

        // Create note for database
        let note_for_db = crate::types::StoredNote {
            header,
            encrypted_data: note.encrypted_details,
            created_at: Utc::now(),
            received_at: Utc::now(),
            received_by: None,
        };

        self.database
            .store_note(&note_for_db)
            .await.map_err(|e| tonic::Status::internal(format!("Failed to store note: {e:?}")))?;

        timer.finish("ok");

        Ok(tonic::Response::new(SendNoteResponse {
            id: note_for_db.header.id().to_hex(),
            status: miden_private_transport_proto::miden_private_transport::NoteStatus::Sent as i32,
        }))
    }

    #[tracing::instrument(skip(self), fields(operation = "grpc.fetch_notes.request"))]
    async fn fetch_notes(
        &self,
        request: tonic::Request<FetchNotesRequest>,
    ) -> Result<tonic::Response<FetchNotesResponse>, tonic::Status> {
        let timer = self.metrics.grpc_fetch_notes_request();

        let request_data = request.into_inner();
        let tag = request_data.tag;

        // Default to epoch start (1970-01-01) to fetch all notes if no timestamp provided
        let timestamp = if let Some(ts) = request_data.timestamp {
            DateTime::from_timestamp(
                ts.seconds,
                ts.nanos.try_into().map_err(|_| {
                    tonic::Status::invalid_argument("Negative timestamp nanoseconds".to_string())
                })?,
            )
            .ok_or_else(|| tonic::Status::invalid_argument("Invalid timestamp"))?
        } else {
            DateTime::from_timestamp(0, 0).unwrap()
        };

        let notes = self
            .database
            .fetch_notes(tag.into(), timestamp)
            .await.map_err(|e| tonic::Status::internal(format!("Failed to fetch notes: {e:?}")))?;

        // Convert to protobuf format
        let proto_notes: Result<Vec<_>, tonic::Status> = notes
            .into_iter()
            .map(|note| {
                let nanos = note.received_at.timestamp_subsec_nanos();
                let nanos_i32 = nanos
                    .try_into()
                    .map_err(|_| tonic::Status::internal("Timestamp nanoseconds too large".to_string()))?;

                Ok(miden_private_transport_proto::miden_private_transport::EncryptedNoteTimestamped {
                    header: note.header.to_bytes(),
                    encrypted_details: note.encrypted_data,
                    timestamp: Some(prost_types::Timestamp {
                        seconds: note.received_at.timestamp(),
                        nanos: nanos_i32,
                    }),
                })
            })
            .collect();
        let proto_notes = proto_notes?;

        timer.finish("ok");

        self.metrics.grpc_fetch_notes_response(
            proto_notes.len() as u64,
            proto_notes.iter().map(|note| (note.header.len() + note.encrypted_details.len()) as u64).sum()
        );

        Ok(tonic::Response::new(FetchNotesResponse { notes: proto_notes }))
    }

    #[tracing::instrument(skip(self), fields(operation = "health"))]
    async fn health(
        &self,
        _request: tonic::Request<()>,
    ) -> Result<tonic::Response<HealthResponse>, tonic::Status> {
        let now = Utc::now();
        let timestamp = prost_types::Timestamp {
            seconds: now.timestamp(),
            nanos: now.timestamp_subsec_nanos()
                    .try_into()
                    .map_err(|_| tonic::Status::internal("Timestamp nanoseconds too large".to_string()))?,
        };

        let response = HealthResponse {
            status: "healthy".to_string(),
            timestamp: Some(timestamp),
            version: env!("CARGO_PKG_VERSION").to_string(),
        };

        tracing::info!(operation = "health", event = "completed", status = "success");
        Ok(tonic::Response::new(response))
    }

    #[tracing::instrument(skip(self), fields(operation = "stats"))]
    async fn stats(
        &self,
        _request: tonic::Request<()>,
    ) -> Result<tonic::Response<StatsResponse>, tonic::Status> {
        let (total_notes, total_tags) = self
            .database
            .get_stats()
            .await.map_err(|e| tonic::Status::internal(format!("Failed to get stats: {e:?}")))?;

        let response = StatsResponse {
            total_notes,
            total_tags,
            notes_per_tag: Vec::new(), // TODO: Implement notes_per_tag
        };

        Ok(tonic::Response::new(response))
    }
}
