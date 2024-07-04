use serde::{Deserialize, Serialize};

pub mod client;
pub mod error;
pub mod types;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BeaconClientConfig {
    pub beacon_client_addresses: Vec<String>,
    pub core: Option<usize>,
}
