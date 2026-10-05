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
//! 3. Tokenized products' deployments from the universe build
//!    (`data/universe/deployments.json`, V1.10): each `REPRESENTS` its
//!    product, asserted by the issuer registry record seeded with the
//!    snapshot. Deployments on chains not stored yet are skipped.
//!
//! Sequential, one request per step, no retries. A fetch failure writes
//! nothing; a record that does not match its binding is rejected whole.

use std::collections::BTreeMap;

use serde::Deserialize;
use sqlx::PgConnection;
use undrly_core::{
    AssetNamespace, Caip2, ChainAsset, CurrencyCode, DeploymentId, DisplayName, InstrumentId, Isin,
    Lei,
};
use undrly_ingest::onchain::{
    ChainBinding, IssuerBinding, IssuerRow, RegistryDeployment, RegistryDeploymentOutcome,
    Tip20Binding, ingest_circle_usdc, ingest_evm_chain, ingest_registry_deployment,
    ingest_solana_chain, ingest_tip20_asset,
};
use undrly_ingest::tracker::{
    TrackerBinding, ingest_tracker_deployment, ingest_tracker_final_terms,
};
use undrly_ingest::{RawRecord, Resolution};
use undrly_provider::http::{FetchedRecord, HttpClient};
use undrly_provider::{circle, evm_rpc, rhj, solana};

use crate::Error;

pub const BINDINGS: &str = "data/reference/onchain.json";
pub const TOKENIZED: &str = "data/reference/tokenized-securities.json";
pub const DEPLOYMENTS: &str = "data/universe/deployments.json";

/// Underlying ISINs of the products bound by reviewed Final Terms: the
/// universe build leaves their registry rows to this command.
pub fn bound_underlyings() -> Result<std::collections::BTreeSet<String>, Error> {
    let tokenized: Tokenized = crate::json(TOKENIZED)?;
    Ok(tokenized
        .products
        .into_iter()
        .map(|p| p.underlying_isin)
        .collect())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Tokenized {
    #[allow(dead_code)]
    dataset: String,
    #[allow(dead_code)]
    version: u32,
    #[allow(dead_code)]
    description: String,
    products: Vec<ProductEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ProductEntry {
    product_name: String,
    product_isin: String,
    issuer_name: String,
    issuer_lei: String,
    underlying_isin: String,
    currency: String,
    relationship: String,
    chain: String,
    final_terms: FinalTermsEntry,
    registry: RegistryEntry,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct FinalTermsEntry {
    source: String,
    url: String,
    sha256: String,
    #[allow(dead_code)]
    retrieved: String,
    #[allow(dead_code)]
    transcribed: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RegistryEntry {
    source: String,
    url: String,
}

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
    #[serde(default)]
    tip20_assets: Vec<Tip20Entry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Tip20Entry {
    chain: String,
    address: String,
    instrument_name: String,
    expect: Tip20Expect,
    source: String,
    #[allow(dead_code)]
    authority: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Tip20Expect {
    name: String,
    symbol: String,
    currency: String,
    decimals: u8,
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
        let binding = ChainBinding {
            name: DisplayName::new(&c.name).map_err(|e| bad("chain name", e))?,
            caip2: Caip2::parse(&c.caip2).map_err(|e| bad("chain", e))?,
        };
        // Each chain is asked for its own identity by its own method.
        let report = match (c.source.as_str(), c.rpc_url.as_str()) {
            (solana::SOURCE_ID, solana::MAINNET_RPC_URL) => {
                let fetched = solana::fetch_genesis_hash(client, &c.rpc_url).await?;
                ingest_solana_chain(conn, &raw(&fetched), &binding).await?
            }
            (evm_rpc::ROBINHOOD_CHAIN_SOURCE_ID, evm_rpc::ROBINHOOD_CHAIN_MAINNET_RPC)
            | (evm_rpc::TEMPO_SOURCE_ID, evm_rpc::TEMPO_MAINNET_RPC)
            | (evm_rpc::BNB_CHAIN_SOURCE_ID, evm_rpc::BNB_CHAIN_MAINNET_RPC) => {
                let fetched = evm_rpc::fetch_chain_id(client, &c.rpc_url).await?;
                ingest_evm_chain(conn, &raw(&fetched), &c.source, &binding).await?
            }
            _ => {
                return Err(bad(
                    "chain",
                    format!("unsupported source {} at {}", c.source, c.rpc_url),
                ));
            }
        };
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
    for t in &bindings.tip20_assets {
        if t.source != evm_rpc::TEMPO_SOURCE_ID {
            return Err(bad(
                "tip20 asset",
                format!("unsupported source {}", t.source),
            ));
        }
        let chain = Caip2::parse(&t.chain).map_err(|e| bad("tip20 chain", e))?;
        let binding = Tip20Binding {
            asset: ChainAsset::new(chain.namespace(), AssetNamespace::Erc20, &t.address)
                .map_err(|e| bad("tip20 address", e))?,
            chain,
            name: t.expect.name.clone(),
            symbol: t.expect.symbol.clone(),
            currency: CurrencyCode::parse(&t.expect.currency)
                .map_err(|e| bad("tip20 currency", e))?,
            decimals: t.expect.decimals,
            instrument_name: DisplayName::new(&t.instrument_name)
                .map_err(|e| bad("tip20 instrument name", e))?,
        };
        let fetched = evm_rpc::fetch_tip20_metadata(
            client,
            evm_rpc::TEMPO_MAINNET_RPC,
            binding.asset.reference(),
        )
        .await?;
        let report = ingest_tip20_asset(conn, &raw(&fetched), &t.source, &binding).await?;
        println!(
            "onchain: TIP-20 {caip19}: {name} {} ({}), deployment {} ({}), TRACKS {}, writes {:?}, record {}",
            report.instrument.id(),
            resolution(&report.instrument),
            report.deployment.id(),
            resolution(&report.deployment),
            t.expect.currency,
            report.writes,
            report.source_record.0.0,
            caip19 = report.caip19,
            name = t.instrument_name,
        );
    }
    deployments(conn).await?;
    products(conn, client).await
}

/// Tokenized products' deployments (V1.10), when the universe was built.
async fn deployments(conn: &mut PgConnection) -> Result<(), Error> {
    if !std::path::Path::new(DEPLOYMENTS).exists() {
        println!("onchain: no {DEPLOYMENTS} (run `universe build`)");
        return Ok(());
    }
    let file: undrly_universe::tokenized::Deployments = crate::json(DEPLOYMENTS)?;
    let bad =
        |what: &str, e: &dyn std::fmt::Display| Error::Usage(format!("{DEPLOYMENTS}: {what}: {e}"));
    let (mut written, mut unchanged, mut no_chain) = (0, 0, BTreeMap::<String, usize>::new());
    for d in &file.deployments {
        let chain = Caip2::parse(&d.chain).map_err(|e| bad("chain", &e))?;
        let namespace =
            AssetNamespace::parse(&d.asset_namespace).map_err(|e| bad("namespace", &e))?;
        let mut sha256 = [0u8; 32];
        if d.sha256.len() != 64 {
            return Err(bad("sha256", &d.sha256));
        }
        for (i, byte) in sha256.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&d.sha256[2 * i..2 * i + 2], 16)
                .map_err(|e| bad("sha256", &e))?;
        }
        let row = RegistryDeployment {
            id: DeploymentId::parse(&d.id).map_err(|e| bad("id", &e))?,
            asset: ChainAsset::new(chain.namespace(), namespace, &d.asset_reference)
                .map_err(|e| bad("asset", &e))?,
            chain,
            product: InstrumentId::parse(&d.represents).map_err(|e| bad("represents", &e))?,
            source: undrly_core::SourceId::parse(&d.source).map_err(|e| bad("source", &e))?,
            record_key: d.record_key.clone(),
            sha256,
        };
        match ingest_registry_deployment(conn, &row).await? {
            RegistryDeploymentOutcome::Written {
                deployment,
                represents,
            } => {
                if deployment == undrly_store::Write::Inserted
                    || represents == undrly_store::Write::Inserted
                {
                    written += 1;
                } else {
                    unchanged += 1;
                }
            }
            RegistryDeploymentOutcome::NoChain => {
                *no_chain.entry(d.chain.clone()).or_default() += 1;
            }
            RegistryDeploymentOutcome::Conflict(other) => println!(
                "onchain: REVIEW: {} is already deployment {other}, not {}",
                d.key, d.id
            ),
        }
    }
    println!(
        "onchain: tokenized deployments: {written} written, {unchanged} unchanged{}",
        if no_chain.is_empty() {
            String::new()
        } else {
            format!("; skipped (chain not stored) {no_chain:?}")
        }
    );
    Ok(())
}

fn resolution<T>(r: &Resolution<T>) -> &'static str {
    match r {
        Resolution::Created(_) => "created",
        Resolution::Existing(_) => "existing",
    }
}

/// Tokenized products (V1.6): each product's Final Terms (reviewed, hash
/// pinned), then its deployment from the issuer's registry.
async fn products(conn: &mut PgConnection, client: &HttpClient) -> Result<(), Error> {
    let tokenized: Tokenized = crate::json(TOKENIZED)?;
    let bad =
        |what: &str, e: &dyn std::fmt::Display| Error::Usage(format!("{TOKENIZED}: {what}: {e}"));
    for p in &tokenized.products {
        if p.relationship != "TRACKS"
            || p.final_terms.source != rhj::FINAL_TERMS_SOURCE_ID
            || p.registry.source != rhj::API_SOURCE_ID
            || p.registry.url != rhj::ASSETS_URL
        {
            return Err(bad(
                "product",
                &format!("unsupported binding {}", p.product_isin),
            ));
        }
        let binding = TrackerBinding {
            final_terms_sha256: p.final_terms.sha256.clone(),
            issuer_name: DisplayName::new(&p.issuer_name).map_err(|e| bad("issuer name", &e))?,
            issuer_lei: Lei::parse(&p.issuer_lei).map_err(|e| bad("issuer LEI", &e))?,
            product_name: DisplayName::new(&p.product_name).map_err(|e| bad("product name", &e))?,
            product_isin: Isin::parse(&p.product_isin).map_err(|e| bad("product ISIN", &e))?,
            underlying_isin: Isin::parse(&p.underlying_isin)
                .map_err(|e| bad("underlying ISIN", &e))?,
            currency: CurrencyCode::parse(&p.currency).map_err(|e| bad("currency", &e))?,
            chain: Caip2::parse(&p.chain).map_err(|e| bad("chain", &e))?,
        };
        let fetched = rhj::fetch_final_terms(client, &p.final_terms.url).await?;
        let report = ingest_tracker_final_terms(conn, &raw(&fetched), &binding).await?;
        let tracks = report.underlying.map_or_else(
            || format!("unresolved ({} is not in Undrly)", p.underlying_isin),
            |u| u.to_string(),
        );
        println!(
            "onchain: {} {}: product {} ({}), issuer {} ({}), TRACKS {tracks}, record {}",
            p.issuer_name,
            p.product_isin,
            report.product.id(),
            resolution(&report.product),
            report.issuer.id(),
            resolution(&report.issuer),
            report.source_record.0.0,
        );
        let fetched = rhj::fetch_assets(client).await?;
        let report = ingest_tracker_deployment(conn, &raw(&fetched), &binding).await?;
        println!(
            "onchain: {} {}: deployment {} ({:?}), REPRESENTS {} ({:?}), record {}",
            p.issuer_name,
            report.caip19,
            report.deployment.id(),
            report.deployment_write,
            p.product_isin,
            report.represents,
            report.source_record.0.0,
        );
        for (id, caip19) in &report.others {
            println!(
                "onchain: REVIEW: {id} {caip19} also represents {} on {}",
                p.product_isin, p.chain
            );
        }
    }
    Ok(())
}
