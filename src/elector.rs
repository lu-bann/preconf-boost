use std::{collections::HashMap, sync::Arc};

use alloy_rpc_types_beacon::BlsPublicKey;
use cb_cli::runner::SignRequestSender;
use cb_common::{pbs::RelayEntry, types::Chain};
use cb_crypto::types::SignRequest;

use cb_pbs::{BuilderEvent, BuilderEventReceiver};
use dashmap::DashSet;
use futures::future::join_all;

use rand::seq::SliceRandom;
use reqwest::Client;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::{
    beacon_client::types::ProposerDuty,
    types::{GatewayElection, SignedGatewayElection, ELECT_GATEWAY_PATH},
    ID,
};

/// Commit module that delegates preconf rights to external gateway
pub struct GatewayElector {
    pub chain: Chain,
    /// List of trusted gateways to choose for election
    pub trusted_gateways: Vec<BlsPublicKey>,
    /// List of relays that support election
    pub relays: Vec<RelayEntry>,

    /// Slot being proposed
    pub next_slot: u64,
    /// The validator pubkeys available
    pub validator_pubkeys: Arc<DashSet<BlsPublicKey>>,
    /// Proposer duties indexed by slot number
    pub duties: HashMap<u64, ProposerDuty>,

    /// Elected gateways by slot
    elections: HashMap<u64, BlsPublicKey>,

    /// Channel to receive proposer duties updates
    duties_rx: mpsc::UnboundedReceiver<Vec<ProposerDuty>>,
}

impl GatewayElector {
    pub fn new(
        chain: Chain,
        trusted_gateways: Vec<BlsPublicKey>,
        relays: Vec<RelayEntry>,

        validator_pubkeys: Arc<DashSet<BlsPublicKey>>,

        duties_rx: mpsc::UnboundedReceiver<Vec<ProposerDuty>>,
    ) -> Self {
        Self {
            chain,
            trusted_gateways,
            relays,

            next_slot: 0,
            validator_pubkeys,
            duties: HashMap::new(),
            elections: HashMap::new(),

            duties_rx,
        }
    }
}

impl GatewayElector {
    pub async fn run(mut self, sign_tx: SignRequestSender) -> eyre::Result<()> {
        while let Some(duties) = self.duties_rx.recv().await {
            self.duties.clear();

            // filter only slots where we're proposer and we havent elected yet
            let our_duties: Vec<_> = duties
                .into_iter()
                .filter(|d| {
                    self.validator_pubkeys.contains(&d.public_key)
                        && !self.elections.contains_key(&d.slot)
                })
                .collect();

            for duty in our_duties {
                // this could be done in parallel
                self.elect_gateway(&sign_tx, duty).await;
            }
        }

        Ok(())
    }

    /// Delegate preconf rights
    async fn elect_gateway(&mut self, sign_tx: &SignRequestSender, duty: ProposerDuty) {
        let gateway_pubkey = *self
            .trusted_gateways
            .choose(&mut rand::thread_rng())
            .unwrap();

        info!(validator_pubkey = %duty.public_key, %gateway_pubkey, slot = duty.slot, "Sending gateway delegation");

        let election_message = GatewayElection {
            gateway_pubkey,
            slot: self.next_slot,
            validator_pubkey: duty.public_key,
            validator_index: duty.validator_index,
        };

        let (request, sign_rx) = SignRequest::new(ID, duty.public_key, election_message.clone());

        if let Err(err) = sign_tx.send(request) {
            error!(?err, "failed sending delegation sign request");
            return;
        };

        match sign_rx.await {
            Ok(Ok(signature)) => {
                let signed_election = SignedGatewayElection {
                    message: election_message,
                    signature,
                };

                let mut handles = Vec::new();

                for relay in &self.relays {
                    let client = Client::new();
                    handles.push(
                        client
                            .post(format!("{}/{ELECT_GATEWAY_PATH}", relay.url))
                            .json(&signed_election)
                            .send(),
                    );
                }

                let results = join_all(handles).await;

                for res in results {
                    match res {
                        Ok(r) => info!("Successful election: {r:?}"),
                        Err(err) => error!("Failed election: {err}"),
                    }
                }
            }

            Ok(Err(err)) => {
                error!(?err, "signature rejected")
            }

            Err(err) => {
                error!(?err, "failed signing")
            }
        }
    }
}

pub async fn run_delegation(
    validator_pubkeys: Arc<DashSet<BlsPublicKey>>,
    mut rx: BuilderEventReceiver,
) -> eyre::Result<()> {
    while let Ok(update) = rx.recv().await {
        if let BuilderEvent::RegisterValidatorRequest(registrations) = update {
            validator_pubkeys.clear();

            for reg in registrations {
                validator_pubkeys.insert(reg.message.pubkey);
            }
        }
    }

    Ok(())
}
