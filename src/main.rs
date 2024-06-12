use beacon_client::client::MultiBeaconClient;
use cb_common::{config::load_module_config, utils::initialize_tracing_log};

use elector::GatewayElector;

use config::PreconfConfig;
use tokio::sync::mpsc;
use tracing::info;

mod beacon_client;
mod config;
mod elector;
mod types;

#[tokio::main]
async fn main() {
    initialize_tracing_log();

    let config = load_module_config::<PreconfConfig>();
    let preconf_config = config.config.extra;

    // start beacon client

    let (beacon_tx, _) = tokio::sync::broadcast::channel(10);
    let multi_beacon_client = MultiBeaconClient::from_endpoint_strs(&preconf_config.beacon_nodes);

    multi_beacon_client
        .subscribe_to_payload_attributes_events(beacon_tx.clone())
        .await;

    let (duties_tx, duties_rx) = mpsc::unbounded_channel();
    tokio::spawn(
        multi_beacon_client.subscribe_to_proposer_duties(duties_tx, beacon_tx.subscribe()),
    );

    info!(module_id = config.config.id, "Starting module");

    let elector = GatewayElector::new(
        config.chain,
        config.config.id,
        format!("http://{}", config.sign_address),
        preconf_config,
        duties_rx,
    );

    elector.run().await;
}
