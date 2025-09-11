#![allow(clippy::cast_precision_loss)]

use std::{
    string::ToString,
    time::{Duration, Instant},
};

use anyhow::Result;
use chrono::Utc;
use miden_objects::utils::Serializable;
use miden_private_transport_client::GrpcClient;
use tokio::{sync::mpsc, time::sleep};

use super::utils::{TagGeneration, generate_dummy_notes};
use crate::{RequestResult, StressMetrics};

#[derive(Clone)]
pub struct GrpcStress {
    endpoint: String,
    workers: usize,
    requests: usize,
    rate: Option<f64>,
}

impl GrpcStress {
    pub fn new(endpoint: String, workers: usize, requests: usize, rate: Option<f64>) -> Self {
        Self { endpoint, workers, requests, rate }
    }

    /// Each worker will run the provided fn `req`
    async fn work<F, Fut>(&self, req: F) -> Result<StressMetrics>
    where
        F: Fn(Self, mpsc::UnboundedSender<RequestResult>) -> Fut + Send + Sync + Clone + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut handles = vec![];

        let start_time = Instant::now();

        // Spawn workers
        for _ in 0..self.workers {
            let cfg = self.clone();
            let req = req.clone();
            let tx = tx.clone();

            let handle = tokio::spawn(async move { req(cfg, tx).await });

            handles.push(handle);
        }

        // Collect results
        let mut total_requests = 0;
        let mut successful_requests = 0;
        let mut failed_requests = 0;
        let mut min_latency = Duration::MAX;
        let mut max_latency = Duration::ZERO;
        let mut total_latency = Duration::ZERO;
        let mut total_size = 0;

        while let Some(result) = rx.recv().await {
            total_requests += 1;

            if result.success {
                successful_requests += 1;
            } else {
                failed_requests += 1;
                println!("Request failed: {:?}", result.error);
            }

            min_latency = min_latency.min(result.latency);
            max_latency = max_latency.max(result.latency);
            total_latency += result.latency;
            total_size += result.size;

            if total_requests >= self.requests {
                break;
            }
        }

        // Wait for all workers to complete
        for handle in handles {
            let _ = handle.await;
        }

        let total_duration = start_time.elapsed();
        let avg_latency = if total_requests > 0 {
            Duration::from_nanos(total_latency.as_nanos() as u64 / total_requests as u64)
        } else {
            Duration::ZERO
        };

        let requests_per_second = if total_duration.as_secs_f64() > 0.0 {
            total_requests as f64 / total_duration.as_secs_f64()
        } else {
            0.0
        };

        let throughput_mbs = if total_size > 0 {
            (total_size as f64 / f64::from(1024 * 1024)) / total_duration.as_secs_f64()
        } else {
            0.0
        };

        Ok(StressMetrics {
            total_requests,
            successful_requests,
            failed_requests,
            total_duration,
            min_latency,
            max_latency,
            avg_latency,
            requests_per_second,
            throughput_mbs,
        })
    }

    pub async fn send_note(&self) -> Result<StressMetrics> {
        println!("Running send-note load test");

        self.work(|cfg, tx| async move {
            let mut client = GrpcClient::connect(cfg.endpoint, 1000).await.unwrap();
            let n_requests = cfg.requests / cfg.workers;
            let notes = generate_dummy_notes(n_requests, &TagGeneration::Sequential(0));

            for (note_header, note_details) in notes {
                let size = note_header.get_size_hint() + note_details.len();

                let start = Instant::now();
                let result = client.send_note(note_header, note_details).await;
                let latency = start.elapsed();

                let success = result.is_ok();
                let error = result.err().map(|e| e.to_string());

                let _ = tx.send(RequestResult { success, latency, error, size });

                // Rate limiting
                if let Some(rate) = cfg.rate {
                    let delay = Duration::from_secs_f64(1.0 / rate);
                    sleep(delay).await;
                }
            }
        })
        .await
    }

    /// `fetch-notes` stress test
    ///
    /// Also populates the server with `n` notes for each tag before fetching them.
    pub async fn fetch_notes(&self, n: usize) -> Result<StressMetrics> {
        println!("Running fetch-notes {n} load test");

        let timestamp = Utc::now();

        println!("Populating...");
        let mut handles = vec![];
        for _ in 0..n {
            let cfg = self.clone();
            let handle = tokio::spawn(async move {
                let mut client = GrpcClient::connect(cfg.endpoint.clone(), 1000).await.unwrap();
                let notes = generate_dummy_notes(cfg.requests, &TagGeneration::Sequential(0));
                for (note_header, note_details) in notes {
                    client.send_note(note_header, note_details).await.unwrap();
                }
            });

            handles.push(handle);
        }

        for handle in handles {
            let _ = handle.await;
        }
        println!("Fetching...");

        self.work(move |cfg, tx| async move {
            let mut client = GrpcClient::connect(cfg.endpoint, 1000).await.unwrap();
            let n_requests = cfg.requests / cfg.workers;

            let mut tag = super::utils::TAG_LOCAL_ANY;
            for _ in 0..n_requests {
                tag += 1;

                let start = Instant::now();
                let result = client.fetch_notes(tag.into(), timestamp).await;
                let latency = start.elapsed();

                let success = result.is_ok();
                let error = result.as_ref().err().map(ToString::to_string);
                let size: usize = result
                    .map(|notes| {
                        notes
                            .iter()
                            .map(|note| note.header.get_size_hint() + note.details.len())
                            .sum()
                    })
                    .unwrap_or(0);

                let _ = tx.send(RequestResult { success, latency, error, size });

                // Rate limiting
                if let Some(rate) = cfg.rate {
                    let delay = Duration::from_secs_f64(1.0 / rate);
                    sleep(delay).await;
                }
            }
        })
        .await
    }

    pub async fn mixed(&self) -> Result<StressMetrics> {
        println!("Running mixed load test (send-note + fetch-notes)");

        let cfg = Self::new(self.endpoint.clone(), self.workers / 2, self.requests / 2, self.rate);

        // Run both tests and combine metrics
        let (send_note_res, fetch_notes_res) = tokio::join!(cfg.send_note(), cfg.fetch_notes(0));
        let (send_note_metrics, fetch_notes_metrics) =
            (send_note_res.unwrap(), fetch_notes_res.unwrap());

        // Combine metrics
        Ok(StressMetrics {
            total_requests: send_note_metrics.total_requests + fetch_notes_metrics.total_requests,
            successful_requests: send_note_metrics.successful_requests
                + fetch_notes_metrics.successful_requests,
            failed_requests: send_note_metrics.failed_requests
                + fetch_notes_metrics.failed_requests,
            total_duration: send_note_metrics
                .total_duration
                .max(fetch_notes_metrics.total_duration),
            min_latency: send_note_metrics.min_latency.min(fetch_notes_metrics.min_latency),
            max_latency: send_note_metrics.max_latency.max(fetch_notes_metrics.max_latency),
            avg_latency: Duration::from_nanos(u128::midpoint(
                send_note_metrics.avg_latency.as_nanos(),
                fetch_notes_metrics.avg_latency.as_nanos(),
            ) as u64),
            requests_per_second: send_note_metrics.requests_per_second
                + fetch_notes_metrics.requests_per_second,
            throughput_mbs: send_note_metrics.throughput_mbs
                + fetch_notes_metrics.requests_per_second,
        })
    }

    pub async fn req_rep(&self) -> Result<StressMetrics> {
        println!("Running req-rep (1-note, send-note -> fetch_notes)");

        self.work(|cfg, tx| async move {
            let mut client = GrpcClient::connect(cfg.endpoint, 1000).await.unwrap();
            let timestamp = Utc::now();
            let n_requests = cfg.requests / cfg.workers;

            let notes = generate_dummy_notes(n_requests, &TagGeneration::Random);

            for (note_header, note_details) in notes {
                let tag = note_header.metadata().tag();
                let start = Instant::now();
                let mut size = note_header.get_size_hint() + note_details.len();

                let mut result = client.send_note(note_header, note_details).await.map(|_| vec![]);
                if result.is_ok() {
                    result = client.fetch_notes(tag, timestamp).await;
                }
                let latency = start.elapsed();

                let success = result.is_ok();
                let error = result.as_ref().err().map(ToString::to_string);
                size += result
                    .map(|notes| {
                        notes
                            .iter()
                            .map(|note| note.header.get_size_hint() + note.details.len())
                            .sum()
                    })
                    .unwrap_or(0);

                let _ = tx.send(RequestResult { success, latency, error, size });

                // Rate limiting
                if let Some(rate) = cfg.rate {
                    let delay = Duration::from_secs_f64(1.0 / rate);
                    sleep(delay).await;
                }
            }
        })
        .await
    }
}
