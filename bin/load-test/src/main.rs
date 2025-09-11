//! Load Testing Tool for Miden Private Transport

#![allow(clippy::cast_precision_loss)]

use std::time::Duration;

use anyhow::Result;
use clap::{Parser, Subcommand};

pub mod grpc;
pub mod utils;

use grpc::GrpcStress;

#[derive(Parser)]
#[command(name = "miden-private-transport-node-load-test")]
#[command(about = "Load testing tool for Miden Private Transport Node")]
struct Args {
    /// Server host
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Server port
    #[arg(long, default_value = "8080")]
    port: u16,

    /// Number of concurrent workers
    #[arg(long, default_value = "10")]
    workers: usize,

    /// Total number of requests to send
    #[arg(long, default_value = "100000")]
    requests: usize,

    /// Test scenario to run
    #[command(subcommand)]
    scenario: Scenario,

    /// Request rate (requests per second per worker)
    #[arg(long)]
    rate: Option<f64>,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Copy, Clone, Debug, Subcommand)]
enum Scenario {
    SendNote,
    FetchNotes {
        /// Fetch `n` notes per request
        n: usize,
    },
    Mixed,
    ReqRep,
}

#[derive(Debug, Clone)]
pub struct StressMetrics {
    total_requests: usize,
    successful_requests: usize,
    failed_requests: usize,
    total_duration: Duration,
    min_latency: Duration,
    max_latency: Duration,
    avg_latency: Duration,
    requests_per_second: f64,
    throughput_mbs: f64,
}

#[derive(Debug)]
struct RequestResult {
    success: bool,
    latency: Duration,
    error: Option<String>,
    size: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let endpoint = format!("http://{}:{}", args.host, args.port);
    println!("Starting load test against: {endpoint}");

    // Run the load test
    let metrics = match args.scenario {
        Scenario::SendNote => {
            GrpcStress::new(endpoint, args.workers, args.requests, args.rate)
                .send_note()
                .await?
        },
        Scenario::FetchNotes { n } => {
            GrpcStress::new(endpoint, args.workers, args.requests, args.rate)
                .fetch_notes(n)
                .await?
        },
        Scenario::Mixed => {
            GrpcStress::new(endpoint, args.workers, args.requests, args.rate)
                .mixed()
                .await?
        },
        Scenario::ReqRep => {
            GrpcStress::new(endpoint, args.workers, args.requests, args.rate)
                .req_rep()
                .await?
        },
    };

    metrics.print(args.scenario);

    Ok(())
}

impl StressMetrics {
    fn print(&self, scenario: Scenario) {
        println!("\n=== {scenario:?} LOAD TEST RESULTS ===");
        println!("Total Requests: {}", self.total_requests);
        println!(
            "Successful: {} ({:.1}%)",
            self.successful_requests,
            (self.successful_requests as f64 / self.total_requests as f64) * 100.0
        );
        println!(
            "Failed: {} ({:.1}%)",
            self.failed_requests,
            (self.failed_requests as f64 / self.total_requests as f64) * 100.0
        );
        println!("Total Duration: {:.2}s", self.total_duration.as_secs_f64());
        println!("Requests/sec: {:.2}", self.requests_per_second);
        println!("Min Latency: {:.2}ms", self.min_latency.as_secs_f64() * 1000.0);
        println!("Max Latency: {:.2}ms", self.max_latency.as_secs_f64() * 1000.0);
        println!("Avg Latency: {:.2}ms", self.avg_latency.as_secs_f64() * 1000.0);
        println!("Throughput (MB/sec): {:.2}", self.throughput_mbs);
        println!("========================");
    }
}
