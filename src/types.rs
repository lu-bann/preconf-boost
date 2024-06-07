use alloy_rpc_types_beacon::{BlsPublicKey, BlsSignature};
use serde::{Deserialize, Serialize};
use tree_hash_derive::TreeHash;

pub const ELECT_GATEWAY_PATH: &str = "/eth/v1/builder/elect_gateway";

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SignedGatewayElection {
    pub message: GatewayElection,
    pub signature: BlsSignature,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, TreeHash)]
pub struct GatewayElection {
    pub gateway_pubkey: BlsPublicKey,
    pub slot: u64,
    pub validator_pubkey: BlsPublicKey,
    pub validator_index: u64,
}
