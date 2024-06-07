use std::sync::Arc;

use beacon_client::client::MultiBeaconClient;
use cb_cli::runner::Runner;
use cb_common::utils::initialize_tracing_log;
use cb_pbs::{BuilderState, DefaultBuilderApi};
use clap::Parser;
use dashmap::DashSet;
use elector::{run_delegation, GatewayElector};

use config::PreconfConfig;
use tokio::sync::mpsc;

mod beacon_client;
mod config;
mod elector;
mod types;

pub const ID: &str = "PRECONFS";

#[tokio::main]
async fn main() {
    initialize_tracing_log();

    // TODO: merge these
    let (chain, config) = cb_cli::Args::parse().to_config();
    let preconf_config = PreconfConfig::load();

    // start beacon client
    let multi_beacon_client = MultiBeaconClient::from_endpoint_strs(&preconf_config.beacon_nodes);
    let (duties_tx, duties_rx) = mpsc::unbounded_channel();
    tokio::spawn(multi_beacon_client.start_proposer_duties_sub(duties_tx));

    let validator_pubkeys = Arc::new(DashSet::new());

    let elector = GatewayElector::new(
        chain,
        preconf_config.trusted_gateways,
        config.relays.clone(),
        validator_pubkeys.clone(),
        duties_rx,
    );

    let state = BuilderState::new(chain, config);
    let mut runner = Runner::<(), DefaultBuilderApi>::new(state);

    let vp = validator_pubkeys.clone();
    runner.add_boost_hook(ID, |rx| async move { run_delegation(vp, rx).await });

    runner.add_commitment(ID, |sign_tx, pubkeys| async move {
        for pubkey in pubkeys {
            validator_pubkeys.insert(pubkey);
        }

        elector.run(sign_tx).await
    });

    if let Err(err) = runner.run().await {
        eprintln!("Error: {err}");
        std::process::exit(1)
    };
}
