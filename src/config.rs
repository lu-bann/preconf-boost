use alloy_rpc_types_beacon::BlsPublicKey;
use cb_common::pbs::RelayEntry;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct PreconfConfig {
    pub relays: Vec<RelayEntry>,
    pub trusted_gateways: Vec<BlsPublicKey>,
    pub beacon_nodes: Vec<String>,
}
