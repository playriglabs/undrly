//! `undrly-collect onchain` (V1.5, docs/v1.5-solana.md): chains and issuer
//! deployments from their authoritative sources, per the reviewed bindings
//! in `data/reference/onchain.json`.
//!
//! 1. For each bound chain: ask the chain's own endpoint for its identity
//!    (Solana: `getGenesisHash`), store the response raw, and ingest the
//!    chain only if it is the bound CAIP-2 chain.
//! 2. For each issuer: fetch its published address list (Circle's USDC
//!    page), store it raw, and ingest a deployment per bound row, each
//!    `REPRESENTS` the bound instrument.
//!
//! Sequential, one request per step, no retries. A fetch failure writes
//! nothing; a record that does not match its binding is rejected whole.

use serde::Deserialize;
use sqlx::PgConnection;
use undrly_core::{AssetNamespace, Caip2, DisplayName, InstrumentId};
use undrly_ingest::onchain::{
    ChainBinding, IssuerBinding, IssuerRow, ingest_circle_usdc, ingest_solana_chain,
};
use undrly_ingest::{RawRecord, Resolution};
use undrly_provider::http::{FetchedRecord, HttpClient};
use undrly_provider::{circle, solana};

use crate::Error;

pub const BINDINGS: &str = "data/reference/onchain.json";

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Bindings {
    #[allow(dead_code)]
    dataset: String,
    #[allow(dead_code)]
    version: u32,
    #[allow(dead_code)]
    description: String,
    chains: Vec<ChainEntry>,
    issuers: Vec<IssuerEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ChainEntry {
    name: String,
    caip2: String,
    source: String,
    rpc_url: String,
    #[allow(dead_code)]
    authority: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct IssuerEntry {
    instrument: String,
    #[allow(dead_code)]
    instrument_name: String,
    #[allow(dead_code)]
    issuer: String,
    source: String,
    url: String,
    #[allow(dead_code)]
    authority: String,
    deployments: Vec<DeploymentEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct DeploymentEntry {
    label: String,
    chain: String,
    asset_namespace: String,
}

fn bad(what: &str, e: impl std::fmt::Display) -> Error {
    Error::Usage(format!("{BINDINGS}: {what}: {e}"))
}

fn raw(f: &FetchedRecord) -> RawRecord {
    RawRecord {
        record_key: f.record_key.clone(),
        payload: f.body.clone(),
        received_at: f.received_at,
    }
}

pub async fn run(conn: &mut PgConnection, client: &HttpClient) -> Result<(), Error> {
    let bindings: Bindings = crate::json(BINDINGS)?;
    for c in &bindings.chains {
        // Only Solana's identity endpoint is implemented.
        if c.source != solana::SOURCE_ID || c.rpc_url != solana::MAINNET_RPC_URL {
            return Err(bad(
                "chain",
                format!("unsupported source {} at {}", c.source, c.rpc_url),
            ));
        }
        let binding = ChainBinding {
            name: DisplayName::new(&c.name).map_err(|e| bad("chain name", e))?,
            caip2: Caip2::parse(&c.caip2).map_err(|e| bad("chain", e))?,
        };
        let fetched = solana::fetch_genesis_hash(client, &c.rpc_url).await?;
        let report = ingest_solana_chain(conn, &raw(&fetched), &binding).await?;
        println!(
            "onchain: {} ({}): chain {} {}, record {} ({:?})",
            c.name,
            binding.caip2,
            report.chain.id(),
            match report.chain {
                Resolution::Created(_) => "created",
                Resolution::Existing(_) => "existing",
            },
            report.source_record.0.0,
            report.source_record.1,
        );
    }
    for i in &bindings.issuers {
        if i.source != circle::SOURCE_ID || i.url != circle::USDC_ADDRESSES_URL {
            return Err(bad(
                "issuer",
                format!("unsupported source {} at {}", i.source, i.url),
            ));
        }
        let binding = IssuerBinding {
            instrument: InstrumentId::parse(&i.instrument).map_err(|e| bad("instrument", e))?,
            deployments: i
                .deployments
                .iter()
                .map(|d| {
                    Ok(IssuerRow {
                        label: d.label.clone(),
                        chain: Caip2::parse(&d.chain).map_err(|e| bad("deployment chain", e))?,
                        namespace: AssetNamespace::parse(&d.asset_namespace)
                            .map_err(|e| bad("asset namespace", e))?,
                    })
                })
                .collect::<Result<_, Error>>()?,
        };
        let fetched = circle::fetch_usdc_addresses(client).await?;
        let report = ingest_circle_usdc(conn, &raw(&fetched), &binding).await?;
        for (id, caip19, deployment, represents) in &report.deployments {
            println!(
                "onchain: {} {caip19}: deployment {} ({deployment:?}), REPRESENTS {} ({represents:?}), record {}",
                i.issuer,
                id.id(),
                binding.instrument,
                report.source_record.0.0,
            );
        }
        for (id, caip19) in &report.others {
            println!(
                "onchain: REVIEW: {id} {caip19} also represents {} on a bound chain but is not in {}'s current list",
                binding.instrument, i.issuer
            );
        }
    }
    Ok(())
}
