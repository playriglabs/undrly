//! Builder tests over **synthetic** inputs in the real formats (made-up
//! assets and holdings, not upstream data).

use std::io::{Cursor, Write};
use std::path::Path;

use undrly_provider::ReferenceDataProvider;
use undrly_provider::curated::CuratedProvider;

use super::*;

fn v1() -> Universe {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data/demo/universe.json");
    CuratedProvider::new()
        .decode_reference(&std::fs::read(path).unwrap())
        .unwrap()
}

fn xlsx(rows: &[Vec<(&str, &str)>]) -> Vec<u8> {
    let mut shared: Vec<String> = Vec::new();
    let mut data = String::new();
    for (i, row) in rows.iter().enumerate() {
        let n = i + 1;
        data.push_str(&format!(r#"<row r="{n}">"#));
        for (col, text) in row {
            let idx = shared.iter().position(|s| s == text).unwrap_or_else(|| {
                shared.push((*text).to_owned());
                shared.len() - 1
            });
            data.push_str(&format!(r#"<c r="{col}{n}" t="s"><v>{idx}</v></c>"#));
        }
        data.push_str("</row>");
    }
    let mut out = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut out);
    let options = zip::write::SimpleFileOptions::default();
    let mut put = |name: &str, body: String| {
        zip.start_file(name, options).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    };
    put(
        "xl/workbook.xml",
        r#"<workbook xmlns:r="r"><sheets><sheet name="holdings" sheetId="1" r:id="rId1"/></sheets></workbook>"#.into(),
    );
    put(
        "xl/_rels/workbook.xml.rels",
        r#"<Relationships><Relationship Id="rId1" Target="worksheets/sheet1.xml"/></Relationships>"#.into(),
    );
    put(
        "xl/sharedStrings.xml",
        format!(
            "<sst>{}</sst>",
            shared
                .iter()
                .map(|s| format!("<si><t>{s}</t></si>"))
                .collect::<String>()
        ),
    );
    put(
        "xl/worksheets/sheet1.xml",
        format!("<worksheet><sheetData>{data}</sheetData></worksheet>"),
    );
    zip.finish().unwrap();
    out.into_inner()
}

fn spy() -> Vec<u8> {
    let holding =
        |name, ticker, cusip| vec![("A", name), ("B", ticker), ("C", cusip), ("H", "USD")];
    xlsx(&[
        vec![("A", "Fund Name:"), ("B", "Synthetic")],
        vec![("A", "Ticker Symbol:"), ("B", "SPY")],
        vec![("A", "Holdings:"), ("B", "As of 23-Sep-2026")],
        vec![
            ("A", "Name"),
            ("B", "Ticker"),
            ("C", "Identifier"),
            ("H", "Local Currency"),
        ],
        holding("NVIDIA CORP", "NVDA", "67066G104"),
        holding("EXAMPLE ONE INC", "EXA", "037833100"),
        // A non-US issuer: imported, but no ISIN is constructed.
        holding("EXAMPLE PLC", "EXP", "G54950103"),
        holding("EXAMPLE CL B", "EXB.B", "594918104"),
        holding("US DOLLAR", "USD", "CASH_USD"),
        holding("OTC CO", "OTCX", "806857108"),
        vec![("A", "Legal footer.")],
    ])
}

fn files() -> BTreeMap<String, Vec<u8>> {
    let mut f = BTreeMap::new();
    let mut put = |p: &str, b: &[u8]| {
        f.insert(p.to_owned(), b.to_vec());
    };
    put(
        paths::MARKETS,
        br#"[
          {"id":"bitcoin","symbol":"btc","name":"Bitcoin","market_cap_rank":1,"last_updated":"2026-09-25T09:12:10.000Z"},
          {"id":"examplecoin","symbol":"exc","name":"Example Coin","market_cap_rank":2,"last_updated":"2026-09-25T09:13:00.000Z"},
          {"id":"krakenonly","symbol":"kro","name":"Kraken Only","market_cap_rank":3,"last_updated":"2026-09-25T09:11:00.000Z"},
          {"id":"lookalike","symbol":"eth","name":"Not Ether","market_cap_rank":4,"last_updated":null},
          {"id":"usd-coin","symbol":"usdc","name":"USDC","market_cap_rank":5,"last_updated":null}
        ]"#,
    );
    put(
        paths::COINS_LIST,
        br#"[{"id":"pepe","symbol":"pepe","name":"Pepe"},{"id":"examplecoin","symbol":"exc","name":"Example Coin"}]"#,
    );
    put(
        &format!("{}001.json", paths::KRAKEN_TICKERS),
        br#"{"tickers":[
          {"base":"XBT","target":"USD","coin_id":"bitcoin"},
          {"base":"EXC","target":"USD","coin_id":"examplecoin"},
          {"base":"EXC","target":"EUR","coin_id":"examplecoin"},
          {"base":"KRO","target":"USD","coin_id":"krakenonly"},
          {"base":"GONE","target":"USD","coin_id":"krakenonly"}
        ]}"#,
    );
    put(
        &format!("{}001.json", paths::COINBASE_TICKERS),
        br#"{"tickers":[
          {"base":"BTC","target":"USD","coin_id":"bitcoin"},
          {"base":"EXC","target":"USD","coin_id":"examplecoin"}
        ]}"#,
    );
    put(
        paths::KRAKEN_ASSET_PAIRS,
        br#"{"error":[],"result":{
          "XXBTZUSD":{"altname":"XBTUSD","wsname":"XBT/USD","status":"online"},
          "EXCUSD":{"altname":"EXCUSD","wsname":"EXC/USD","status":"online"},
          "KROUSD":{"altname":"KROUSD","wsname":"KRO/USD","status":"online"},
          "ETHUSD":{"altname":"ETHUSD","wsname":"ETH/USD","status":"online"}
        }}"#,
    );
    put(
        paths::COINBASE_PRODUCTS,
        br#"[{"id":"BTC-USD","base_currency":"BTC","quote_currency":"USD","status":"online"},
             {"id":"EXC-USD","base_currency":"EXC","quote_currency":"USD","status":"online"},
             {"id":"ETH-USD","base_currency":"ETH","quote_currency":"USD","status":"online"}]"#,
    );
    put(
        paths::HYPERLIQUID_META,
        br#"[{"universe":[{"name":"BTC"},{"name":"kPEPE"},{"name":"EXC"},{"name":"NOPE"},{"name":"OLD","isDelisted":true},{"name":"HYPE"}],"collateralToken":0},
             [{"markPx":"1"},{"markPx":"1"},{"markPx":"1"},{"markPx":"1"},{"markPx":"1"},{"markPx":"1"}]]"#,
    );
    put(
        paths::HYPERLIQUID_DERIVATIVES,
        br#"{"tickers":[
          {"symbol":"BTC-USD","base":"BTC","target":"USD","coin_id":"bitcoin","contract_type":"perpetual"},
          {"symbol":"KPEPE-USD","base":"KPEPE","target":"USD","coin_id":"pepe","contract_type":"perpetual"},
          {"symbol":"EXC-USD","base":"EXC","target":"USD","coin_id":"examplecoin","contract_type":"perpetual"},
          {"symbol":"XYZ:NOPE-USD","base":"XYZ:NOPE","target":"USD","coin_id":"examplecoin","contract_type":"perpetual"}
        ]}"#,
    );
    put(paths::SPY_HOLDINGS, &spy());
    put(
        paths::QQQ_HOLDINGS,
        br#"{"effectiveBusinessDate":"2026-10-02","holdings":[
          {"ticker":"NVDA","issuerName":"NVIDIA Corp","cusip":"67066G104","currency":"USD","securityTypeCode":"COM"},
          {"ticker":"EXA","issuerName":"Example One Inc","cusip":"037833100","currency":"USD","securityTypeCode":"COM"},
          {"ticker":"NEWQ","issuerName":"Nasdaq Only Inc","cusip":"02079K305","currency":"USD","securityTypeCode":"ADR"},
          {"ticker":"NQZ6","issuerName":"Index future","cusip":null,"currency":"USD","securityTypeCode":"IFUT"}]}"#,
    );
    put(
        paths::SEC_TICKERS,
        br#"{"fields":["cik","name","ticker","exchange"],"data":[
          [1045810,"NVIDIA CORP","NVDA","Nasdaq"],
          [111,"Example One Inc","EXA","Nasdaq"],
          [222,"Example plc","EXP","NYSE"],
          [333,"Example Holdings","EXB-B","NYSE"],
          [444,"OTC Co","OTCX","OTC"],
          [555,"Nasdaq Only Inc","NEWQ","Nasdaq"]]}"#,
    );
    f
}

fn manifest(files: &BTreeMap<String, Vec<u8>>) -> Manifest {
    Manifest {
        files: files
            .iter()
            .map(|(path, bytes)| ManifestFile {
                path: path.clone(),
                source: path.split('/').next().unwrap().to_owned(),
                record_key: format!("https://example.test/{path}"),
                sha256: sha256_hex(bytes),
                bytes: bytes.len(),
                fetched_at: "2026-09-25T10:00:00Z".into(),
            })
            .collect(),
    }
}

/// Deterministic ids for tests.
fn counter() -> impl FnMut(Category) -> CanonicalId {
    let mut n: u64 = 0;
    move |category| {
        n += 1;
        let uuid = format!("01900000-0000-7000-8000-{n:012x}");
        CanonicalId::from_parts(category, uuid.parse().unwrap()).unwrap()
    }
}

fn run(ids: IdMap) -> Build {
    let files = files();
    let manifest = manifest(&files);
    let v1 = v1();
    let inputs = Inputs {
        manifest: &manifest,
        files: &files,
        v1: &v1,
        ids,
        bound_underlyings: &BTreeSet::new(),
        external: &BTreeMap::new(),
    };
    build(&inputs, &mut counter()).unwrap()
}

#[test]
fn build_is_deterministic_and_keeps_ids() {
    let a = run(IdMap::default());
    let b = run(IdMap::default());
    assert_eq!(to_json(&a.snapshot), to_json(&b.snapshot));
    assert_eq!(a.report, b.report);
    // Rebuilding with the produced map mints nothing and changes nothing.
    let c = run(a.ids.clone());
    assert_eq!(to_json(&a.snapshot), to_json(&c.snapshot));
    assert_eq!(a.ids, c.ids);
    assert!(c.report.contains("ids minted this build: 0"));
    // The snapshot is valid curated input.
    let decoded = CuratedProvider::new()
        .decode_reference(&to_json(&a.snapshot))
        .unwrap();
    assert_eq!(decoded, a.snapshot);
}

#[test]
fn crypto_maps_through_the_crosswalk_only() {
    let b = run(IdMap::default());
    let v1 = v1();
    let btc = v1
        .instruments
        .iter()
        .find(|i| i.key == "btc")
        .unwrap()
        .id
        .clone();
    let feeds: Vec<(&str, &str, &str)> = b
        .snapshot
        .quote_feeds
        .iter()
        .filter(|f| f.source == "kraken" || f.source == "coinbase")
        .map(|f| (f.source.as_str(), f.symbol.as_str(), f.subject.as_str()))
        .collect();
    assert_eq!(
        feeds,
        vec![
            ("coinbase", "EXC-USD", "coingecko:examplecoin"),
            ("kraken", "EXCUSD", "coingecko:examplecoin"),
            ("kraken", "KROUSD", "coingecko:krakenonly"),
        ],
        "V1 BTC feeds are not redeclared; `lookalike` (symbol ETH) gets no ETH market"
    );
    // V1 Bitcoin is reused, not redeclared.
    assert!(b.snapshot.instruments.iter().all(|i| i.id != btc));
    assert_eq!(b.snapshot.universes[0].key, "crypto-top100");
    assert_eq!(b.snapshot.universes[0].members[0].node, btc);
    assert_eq!(b.snapshot.universes[0].as_of, "2026-09-25T09:13:00.000Z");
    // Two venues → mean-venue-mid-v1; BTC's declaration stays V1's.
    let means: Vec<&str> = b
        .snapshot
        .quote_aggregations
        .iter()
        .filter(|a| a.method == "mean-venue-mid-v1")
        .map(|a| a.subject.as_str())
        .collect();
    assert_eq!(means, vec!["coingecko:examplecoin"]);
    // Every non-V1 perpetual: its mark with its venue book.
    let perps: Vec<&str> = b
        .snapshot
        .quote_aggregations
        .iter()
        .filter(|a| a.method == "mark-with-venue-book-v1")
        .map(|a| a.subject.as_str())
        .collect();
    assert_eq!(
        perps,
        vec![
            "hyperliquid:EXC",
            "hyperliquid:HYPE",
            "hyperliquid:NOPE",
            "hyperliquid:kPEPE"
        ]
    );
    assert!(b.report.contains("GONE/USD"));
    assert!(
        b.report
            .contains("lookalike: no crosswalked Kraken, Coinbase or Binance market")
    );
}

#[test]
fn perps_are_discovered_with_multipliers_and_crosswalked_underlyings() {
    let b = run(IdMap::default());
    let perps = b
        .snapshot
        .universes
        .iter()
        .find(|u| u.key == "hyperliquid-perps")
        .unwrap();
    let symbols: Vec<&str> = perps
        .members
        .iter()
        .map(|m| m.source_symbol.as_deref().unwrap())
        .collect();
    assert_eq!(
        symbols,
        vec!["BTC", "EXC", "HYPE", "NOPE", "kPEPE"],
        "delisted OLD skipped"
    );
    let kpepe = b
        .snapshot
        .instruments
        .iter()
        .find(|i| i.key == "hyperliquid:kPEPE")
        .unwrap();
    assert_eq!(kpepe.contract_multiplier.as_deref(), Some("1000"));
    assert_eq!(contract_multiplier("KAITO"), None);
    assert_eq!(contract_multiplier("k"), None);
    let edge = |s: &str| {
        b.snapshot
            .relationships
            .iter()
            .find(|(a, t, _)| a == s && t == "DERIVES_FROM")
            .map(|(_, _, o)| o.clone())
    };
    // The underlying outside the top 100 is created from coins/list.
    assert_eq!(edge("hyperliquid:kPEPE").as_deref(), Some("coingecko:pepe"));
    assert_eq!(
        edge("hyperliquid:EXC").as_deref(),
        Some("coingecko:examplecoin")
    );
    // HIP-3 (`XYZ:NOPE`) is not a main market: no edge.
    assert_eq!(edge("hyperliquid:NOPE"), None);
    assert!(
        b.report
            .contains("NOPE: no CoinGecko derivatives crosswalk")
    );
}

#[test]
fn equities_get_no_constructed_identifiers() {
    let b = run(IdMap::default());
    let snapshot = to_json(&b.snapshot);
    let text = String::from_utf8(snapshot).unwrap();
    assert!(!text.contains("\"isin\""), "no ISIN is ever emitted");
    for cusip in ["037833100", "G54950103"] {
        assert!(
            !text.contains(&format!("\"{cusip}\"")),
            "CUSIPs appear only inside build keys"
        );
    }
    let equities: Vec<&str> = b
        .snapshot
        .instruments
        .iter()
        .filter(|i| i.class == "equity")
        .map(|i| i.name.as_str())
        .collect();
    assert_eq!(
        equities,
        vec![
            "Nasdaq Only Inc",
            "EXAMPLE ONE INC",
            "EXAMPLE CL B",
            "EXAMPLE PLC"
        ]
    );
    // Issuers carry their CIK; NVIDIA's V1 entity is reused.
    let ciks: Vec<&str> = b
        .snapshot
        .entities
        .iter()
        .map(|e| e.cik.as_deref().unwrap())
        .collect();
    assert_eq!(
        ciks,
        vec!["0000000111", "0000000222", "0000000333", "0000000555"]
    );
    let venues: Vec<&str> = b
        .snapshot
        .venues
        .iter()
        .map(|v| v.mic.as_deref().unwrap())
        .collect();
    assert_eq!(venues, vec!["XNYS"]);
    // The common venue name is a symbol alias; the MIC stays the identifier.
    let venue_symbols: Vec<&str> = b
        .snapshot
        .aliases
        .iter()
        .filter(|a| a.node == "venue:XNYS" && a.kind == "symbol")
        .map(|a| a.alias.as_str())
        .collect();
    assert_eq!(venue_symbols, vec!["NYSE", "XNYS"]);
    let class_b = b
        .snapshot
        .listings
        .iter()
        .find(|l| l.symbol == "EXB.B")
        .unwrap();
    assert_eq!(class_b.venue, "venue:XNYS");
    let sp = b
        .snapshot
        .universes
        .iter()
        .find(|u| u.key == "sp500")
        .unwrap();
    assert_eq!(sp.members.len(), 4);
    // Equities in the default inputs: SPY's four plus QQQ's NEWQ.
    assert_eq!(sp.as_of, "2026-09-23T00:00:00Z");
    assert!(
        b.report
            .contains("OTCX: SEC exchange Some(\"OTC\") has no MIC rule")
    );
    assert!(b.report.contains("is not a valid CUSIP"));
    // Nasdaq-100 through QQQ: S&P members are reused by CUSIP, the others
    // imported the same way; non-share rows are skipped.
    let ndx = b
        .snapshot
        .universes
        .iter()
        .find(|u| u.key == "nasdaq100")
        .unwrap();
    let symbols: Vec<&str> = ndx
        .members
        .iter()
        .map(|m| m.source_symbol.as_deref().unwrap())
        .collect();
    assert_eq!(symbols, vec!["EXA", "NEWQ", "NVDA"]);
    assert_eq!(ndx.as_of, "2026-10-02T00:00:00Z");
    let exa = &ndx.members[0].node;
    assert_eq!(exa, "spy:037833100", "one instrument for both ETFs");
    assert!(
        b.snapshot
            .instruments
            .iter()
            .any(|i| i.key == "spy:02079K305" && i.name == "Nasdaq Only Inc")
    );
    assert!(
        b.report
            .contains("NQZ6: security type \"IFUT\" is not a share")
    );
}

#[test]
fn nasdaq_list_is_optional() {
    let mut files = files();
    files.remove(paths::QQQ_HOLDINGS);
    let manifest = manifest(&files);
    let v1 = v1();
    let inputs = Inputs {
        manifest: &manifest,
        files: &files,
        v1: &v1,
        ids: IdMap::default(),
        bound_underlyings: &BTreeSet::new(),
        external: &BTreeMap::new(),
    };
    let b = build(&inputs, &mut counter()).unwrap();
    assert!(b.snapshot.universes.iter().all(|u| u.key != "nasdaq100"));
    assert!(b.report.contains("nasdaq100: not built"));
    assert!(b.deployments.deployments.is_empty());
}

#[test]
fn rejects_tampered_inputs() {
    let mut files = files();
    let manifest = manifest(&files);
    files.get_mut(paths::MARKETS).unwrap().push(b' ');
    let v1 = v1();
    let inputs = Inputs {
        manifest: &manifest,
        files: &files,
        v1: &v1,
        ids: IdMap::default(),
        bound_underlyings: &BTreeSet::new(),
        external: &BTreeMap::new(),
    };
    assert!(matches!(
        build(&inputs, &mut counter()),
        Err(BuildError::Hash { .. })
    ));
}

/// Hyperliquid's contract specification: USDT-denominated (PURR and HYPE:
/// USDC-denominated), USDC-margined, profit and loss in USDC.
#[test]
fn perps_carry_their_documented_denomination_margin_and_settlement() {
    let b = run(IdMap::default());
    let v1 = v1();
    let id = |k: &str| {
        v1.instruments
            .iter()
            .find(|i| i.key == k)
            .unwrap()
            .id
            .clone()
    };
    let (usdc, usdt) = (id("usdc"), id("usdt"));
    assert_ne!(usdc, usdt);
    let edges = |s: &str, t: &str| -> Vec<String> {
        b.snapshot
            .relationships
            .iter()
            .filter(|(a, k, _)| a == s && k == t)
            .map(|(_, _, o)| o.clone())
            .collect()
    };
    let unit = |symbol: &str| {
        b.snapshot
            .quote_feeds
            .iter()
            .find(|f| f.source == "hyperliquid" && f.symbol == symbol)
            .map(|f| f.unit.clone())
            .unwrap()
    };
    for perp in ["EXC", "NOPE", "kPEPE"] {
        let key = format!("hyperliquid:{perp}");
        assert_eq!(edges(&key, "DENOMINATED_IN"), vec![usdt.clone()], "{perp}");
        assert_eq!(edges(&key, "MARGINED_IN"), vec![usdc.clone()], "{perp}");
        assert_eq!(edges(&key, "SETTLES_IN"), vec![usdc.clone()], "{perp}");
        assert_eq!(unit(perp), usdt, "{perp}: the mark is in the denomination");
    }
    // The documented USDC-denominated exception.
    assert_eq!(
        edges("hyperliquid:HYPE", "DENOMINATED_IN"),
        vec![usdc.clone()]
    );
    assert_eq!(unit("HYPE"), usdc);
    // V1's BTC perpetual carries its own edges (data/demo/universe.json); its
    // feed is V1's, in USDT.
    assert!(edges("hyperliquid:BTC", "DENOMINATED_IN").is_empty());
    let btc_feed = v1
        .quote_feeds
        .iter()
        .find(|f| f.source == "hyperliquid" && f.symbol == "BTC")
        .unwrap();
    assert_eq!(btc_feed.unit, "usdt");
    // The multiplier is unchanged by any of this.
    let kpepe = b
        .snapshot
        .instruments
        .iter()
        .find(|i| i.key == "hyperliquid:kPEPE")
        .unwrap();
    assert_eq!(kpepe.contract_multiplier.as_deref(), Some("1000"));
}

/// Without the documented collateral token, no margin or settlement asset
/// is asserted; the price unit (from the specification) still is.
#[test]
fn perps_without_the_documented_collateral_get_no_margin_edges() {
    let mut files = files();
    files.insert(
        paths::HYPERLIQUID_META.to_owned(),
        br#"[{"universe":[{"name":"EXC"}],"collateralToken":7},[{"markPx":"1"}]]"#.to_vec(),
    );
    let manifest = manifest(&files);
    let v1 = v1();
    let inputs = Inputs {
        manifest: &manifest,
        files: &files,
        v1: &v1,
        ids: IdMap::default(),
        bound_underlyings: &BTreeSet::new(),
        external: &BTreeMap::new(),
    };
    let b = build(&inputs, &mut counter()).unwrap();
    let kinds: Vec<&str> = b
        .snapshot
        .relationships
        .iter()
        .filter(|(a, _, _)| a == "hyperliquid:EXC")
        .map(|(_, k, _)| k.as_str())
        .collect();
    assert!(kinds.contains(&"DENOMINATED_IN"));
    assert!(!kinds.contains(&"MARGINED_IN"));
    assert!(!kinds.contains(&"SETTLES_IN"));
    assert!(b.report.contains("collateralToken is Some(7)"));
}

const MINT_A: &str = "XsbEhLAtcf6HdfpFZ5xEMdqW8nfAvcsP5bdudRLJzJp";
const MINT_B: &str = "AAPLEDt8RpzPgXyhvFzkMBofvFSQw9gpeMCoUdPdLnB8";
const RHJ_ADDRESS: &str = "0xd0601CE157Db5bdC3162BbaC2a2C8aF5320D9EEC";

fn with_registries() -> BTreeMap<String, Vec<u8>> {
    let mut f = files();
    f.insert(
        paths::BACKED_TOKENS.into(),
        format!(
            r#"{{"nodes":[
              {{"name":"Example One xStock","symbol":"EXAx","isin":"CH1436219195",
                "underlyingSymbol":"EXA","underlyingIsin":"US0378331005",
                "deployments":[{{"address":"0xec901b1779eac0bb26bb4d2908ad1674f3e9beb7","network":"Ethereum"}},
                               {{"address":"svm:{MINT_A}","network":"Solana"}}]}},
              {{"name":"Wrong Ticker xStock","symbol":"WRONGx","isin":"CH1173294336",
                "underlyingSymbol":"WRONG","underlyingIsin":"US0378331005","deployments":[]}},
              {{"name":"Irish xStock","symbol":"IEx","isin":"CH1588660659",
                "underlyingSymbol":"A5GI","underlyingIsin":"IE00BF0L3536","deployments":[]}}],
              "page":{{"currentPage":0,"hasNextPage":false}}}}"#
        )
        .into_bytes(),
    );
    f.insert(
        paths::BACKPACK_SECURITIES.into(),
        br#"[{"asset":"EXA.US","cusip":"037833100","name":"Example One","sessions":[]},
             {"asset":"NOID.US","cusip":null,"name":"No Identifier","sessions":[]}]"#
            .to_vec(),
    );
    f.insert(
        paths::BACKPACK_ASSETS.into(),
        format!(
            r#"[{{"symbol":"EXA.US","displayName":"Example One","tokens":[
                  {{"blockchain":"Solana","contractAddress":"{MINT_B}"}}]}}]"#
        )
        .into_bytes(),
    );
    f.insert(
        paths::RHJ_ASSETS.into(),
        format!(
            r#"{{"assets":[
              {{"id":"0x01","tokenSymbol":"NVDA","tokenName":"NVIDIA • Robinhood Token","status":"ASSET_STATUS_ACTIVE",
                "isin":"US67066G1040","deployments":[{{"contractAddress":"{RHJ_ADDRESS}","chainId":4663}}]}},
              {{"id":"0x02","tokenSymbol":"EXA","tokenName":"Example One • Robinhood Token","status":"ASSET_STATUS_ACTIVE",
                "isin":"US0378331005","deployments":[{{"contractAddress":"0x9443176f5224ef669847524b35eb5c1d80ecac3f","chainId":4663}}]}}]}}"#
        )
        .into_bytes(),
    );
    f
}

#[test]
fn tokenized_stocks_are_distinct_products_tied_to_shares_by_identifier() {
    let files = with_registries();
    let manifest = manifest(&files);
    let v1 = v1();
    let bound: BTreeSet<String> = ["US67066G1040".to_owned()].into();
    let inputs = Inputs {
        manifest: &manifest,
        files: &files,
        v1: &v1,
        ids: IdMap::default(),
        bound_underlyings: &bound,
        external: &BTreeMap::new(),
    };
    let b = build(&inputs, &mut counter()).unwrap();
    let products: Vec<(&str, Option<&str>)> = b
        .snapshot
        .instruments
        .iter()
        .filter(|i| i.class == "tokenized_security")
        .map(|i| (i.key.as_str(), i.isin.as_deref()))
        .collect();
    let backpack_key = format!("backpack:{}/token:{MINT_B}", tokenized::SOLANA);
    assert_eq!(
        products,
        vec![
            ("backed:CH1436219195", Some("CH1436219195")),
            (backpack_key.as_str(), None),
            ("rhj:0x02", None),
        ]
    );
    let edges: Vec<(&str, &str, &str)> = b
        .snapshot
        .relationships
        .iter()
        .filter(|(s, _, _)| {
            s.starts_with("backed:") || s.starts_with("backpack:") || s.starts_with("rhj:")
        })
        .map(|(s, t, o)| (s.as_str(), t.as_str(), o.as_str()))
        .collect();
    assert_eq!(
        edges,
        vec![
            (
                "backed:CH1436219195",
                "ISSUED_BY",
                "lei:984500001AB7C6C7F577"
            ),
            ("backed:CH1436219195", "TRACKS", "spy:037833100"),
            (backpack_key.as_str(), "TOKENIZES", "spy:037833100"),
            ("rhj:0x02", "ISSUED_BY", "lei:984500ADFHQZ9D6B9A29"),
            ("rhj:0x02", "TRACKS", "spy:037833100"),
        ],
        "never ISSUED_BY the share's issuer; Backpack's SPV is unnamed"
    );
    // The share gains the ISIN its tokens' issuers state; it keeps its id.
    let exa = b
        .snapshot
        .instruments
        .iter()
        .find(|i| i.key == "spy:037833100")
        .unwrap();
    assert_eq!(exa.isin.as_deref(), Some("US0378331005"));
    // A token is never aliased with the share's ticker.
    assert!(
        !b.snapshot
            .aliases
            .iter()
            .any(|a| a.node.starts_with("rhj:") && a.kind == "symbol")
    );
    // Deployments: Solana (Backed, Backpack) and Robinhood Chain; Ethereum
    // is not a chain Undrly has.
    let d: Vec<(&str, &str, &str)> = b
        .deployments
        .deployments
        .iter()
        .map(|d| {
            (
                d.chain.as_str(),
                d.asset_reference.as_str(),
                d.source.as_str(),
            )
        })
        .collect();
    assert_eq!(d.len(), 3);
    assert!(d.contains(&(tokenized::SOLANA, MINT_A, "backed")));
    assert!(d.contains(&(tokenized::SOLANA, MINT_B, "backpack")));
    assert!(d.contains(&(
        tokenized::ROBINHOOD_CHAIN,
        "0x9443176f5224ef669847524b35eb5c1d80ecac3f",
        "rhj"
    )));
    // Solana deployments are priced by Jupiter, keyed by mint.
    let jupiter: Vec<(&str, &str)> = b
        .snapshot
        .quote_feeds
        .iter()
        .filter(|f| f.source == "jupiter")
        .map(|f| (f.symbol.as_str(), f.subject.as_str()))
        .collect();
    assert_eq!(
        jupiter,
        vec![
            (MINT_B, backpack_key.as_str()),
            (MINT_A, "backed:CH1436219195")
        ]
    );
    assert!(
        b.report
            .contains("registry's ticker is Some(\"WRONG\"): rejected")
    );
    assert!(
        b.report
            .contains("IEx (Some(\"IE00BF0L3536\")): no US ISIN or CUSIP")
    );
    assert!(b.report.contains("1 securities: Backpack states no CUSIP"));
    assert!(b.report.contains("reviewed Final Terms"));
    // Deterministic, and the snapshot stays valid curated input.
    let again = build(&inputs, &mut counter()).unwrap();
    assert_eq!(to_json(&b.deployments), to_json(&again.deployments));
    CuratedProvider::new()
        .decode_reference(&to_json(&b.snapshot))
        .unwrap();
}

#[test]
fn an_asset_declared_by_another_file_is_reused_not_redeclared() {
    let files = files();
    let manifest = manifest(&files);
    let v1 = v1();
    let theirs = "undrly:instrument:01m3v97hfdejxt2xbckxvnpjc9".to_owned();
    let external: BTreeMap<String, String> =
        [("coingecko:examplecoin".to_owned(), theirs.clone())].into();
    let inputs = Inputs {
        manifest: &manifest,
        files: &files,
        v1: &v1,
        ids: IdMap::default(),
        bound_underlyings: &BTreeSet::new(),
        external: &external,
    };
    let b = build(&inputs, &mut counter()).unwrap();
    assert!(b.snapshot.instruments.iter().all(|i| i.id != theirs));
    assert!(
        b.snapshot
            .quote_feeds
            .iter()
            .any(|f| f.symbol == "EXCUSD" && f.subject == theirs),
        "its feeds name it by id"
    );
    assert_eq!(b.ids.ids["coingecko:examplecoin"], theirs);
    // A map that already gave the key another id is an error, never a merge.
    let mut ids = IdMap::default();
    ids.ids.insert(
        "coingecko:examplecoin".into(),
        "undrly:instrument:01m43n88vseaybmp0h55g52wc6".into(),
    );
    let inputs = Inputs { ids, ..inputs };
    assert!(matches!(
        build(&inputs, &mut counter()),
        Err(BuildError::IdMap(_))
    ));
}

#[test]
fn bstocks_are_tokens_of_their_issuer_with_no_edge_to_a_share() {
    let mut files = files();
    files.insert(
        paths::BSTOCKS.into(),
        br#"{"code":"000000","data":[
          {"chainId":"56","contractAddress":"0xcdf2f3e0fa43c47a6662a91c9e4a7c5f69762699","symbol":"EXAB",
           "ticker":"EXA","type":3,"cs":"EXABUSDT","asset":"EXAB"},
          {"chainId":"56","contractAddress":"0x80f3d493ebce97e343c53d29a137942416b4ffc0","symbol":"NOBOOKB",
           "ticker":"NOBOOK","type":3,"cs":"NOBOOKBUSDT","asset":"NOBOOKB"}]}"#
            .to_vec(),
    );
    files.insert(
        paths::BINANCE_EXCHANGE_INFO.into(),
        br#"{"symbols":[{"symbol":"EXABUSDT","status":"TRADING","baseAsset":"EXAB","quoteAsset":"USDT"}]}"#
            .to_vec(),
    );
    let manifest = manifest(&files);
    let v1 = v1();
    let binance = "undrly:venue:01m3v97hfdejxt2xbck4ymbf5y".to_owned();
    let external: BTreeMap<String, String> = [("venue:binance".to_owned(), binance.clone())].into();
    let inputs = Inputs {
        manifest: &manifest,
        files: &files,
        v1: &v1,
        ids: IdMap::default(),
        bound_underlyings: &BTreeSet::new(),
        external: &external,
    };
    let b = build(&inputs, &mut counter()).unwrap();
    let key = "bstocks:eip155:56/erc20:0xcdf2f3e0fa43c47a6662a91c9e4a7c5f69762699";
    let edges: Vec<(&str, &str)> = b
        .snapshot
        .relationships
        .iter()
        .filter(|(s, _, _)| s == key)
        .map(|(_, t, o)| (t.as_str(), o.as_str()))
        .collect();
    assert_eq!(
        edges,
        vec![
            ("ISSUED_BY", "lei:25490057XTWK16I5YJ22"),
            ("TRADES_ON", binance.as_str())
        ],
        "no TRACKS/TOKENIZES from a ticker"
    );
    let feeds: Vec<(&str, &str)> = b
        .snapshot
        .quote_feeds
        .iter()
        .filter(|f| f.source == "binance")
        .map(|f| (f.symbol.as_str(), f.subject.as_str()))
        .collect();
    assert_eq!(feeds, vec![("EXABUSDT", key)]);
    assert_eq!(b.deployments.deployments.len(), 2);
    assert!(
        b.report
            .contains("NOBOOKB: spot market Some(\"NOBOOKBUSDT\") not trading")
    );
}

#[test]
fn wider_universes_top500_binance_markets_and_usd_conversion() {
    let mut files = files();
    let mut put = |p: &str, b: &[u8]| {
        files.insert(p.to_owned(), b.to_vec());
    };
    // Page 2: a new coin, and one that moved up between the two requests.
    put(
        paths::MARKETS_PAGE2,
        br#"[{"id":"binanceonly","symbol":"bno","name":"Binance Only","market_cap_rank":251,"last_updated":"2026-09-25T09:14:00.000Z"},
             {"id":"examplecoin","symbol":"exc","name":"Example Coin","market_cap_rank":252,"last_updated":null}]"#,
    );
    put(
        &format!("{}001.json", paths::BINANCE_TICKERS),
        br#"{"tickers":[{"base":"BNO","target":"USDT","coin_id":"binanceonly"},
                        {"base":"EXC","target":"USDT","coin_id":"examplecoin"},
                        {"base":"EXC","target":"BTC","coin_id":"examplecoin"}]}"#,
    );
    put(
        paths::BINANCE_EXCHANGE_INFO,
        br#"{"symbols":[{"symbol":"BNOUSDT","status":"TRADING","baseAsset":"BNO","quoteAsset":"USDT"},
                        {"symbol":"EXCUSDT","status":"TRADING","baseAsset":"EXC","quoteAsset":"USDT"}]}"#,
    );
    put(
        paths::MDY_HOLDINGS,
        &xlsx(&[
            vec![("A", "Ticker Symbol:"), ("B", "MDY")],
            vec![("A", "Holdings:"), ("B", "As of 23-Sep-2026")],
            vec![
                ("A", "Name"),
                ("B", "Ticker"),
                ("C", "Identifier"),
                ("H", "Local Currency"),
            ],
            vec![
                ("A", "Nasdaq Only Inc"),
                ("B", "NEWQ"),
                ("C", "02079K305"),
                ("H", "USD"),
            ],
            vec![
                ("A", "EXAMPLE ONE INC"),
                ("B", "EXA"),
                ("C", "037833100"),
                ("H", "USD"),
            ],
        ]),
    );
    let manifest = manifest(&files);
    let v1 = v1();
    let binance = "undrly:venue:01m3v97hfdejxt2xbck4ymbf5y".to_owned();
    let external: BTreeMap<String, String> = [("venue:binance".to_owned(), binance.clone())].into();
    let inputs = Inputs {
        manifest: &manifest,
        files: &files,
        v1: &v1,
        ids: IdMap::default(),
        bound_underlyings: &BTreeSet::new(),
        external: &external,
    };
    let b = build(&inputs, &mut counter()).unwrap();
    let universe = |k: &str| b.snapshot.universes.iter().find(|u| u.key == k).unwrap();
    // Top 500: both pages, a coin in both once.
    assert_eq!(universe("crypto-top500").members.len(), 6);
    assert_eq!(
        universe("crypto-top500").record_key,
        "https://example.test/coingecko/markets-002.json"
    );
    assert_eq!(universe("crypto-top250").members.len(), 6, "only 6 ranked");
    // Binance USDT markets: their own market (Tether), never averaged with USD.
    let usdt = v1
        .instruments
        .iter()
        .find(|i| i.key == "usdt")
        .unwrap()
        .id
        .clone();
    let binance_spot: Vec<(&str, &str, &str)> = b
        .snapshot
        .quote_feeds
        .iter()
        .filter(|f| f.source == "binance")
        .map(|f| (f.symbol.as_str(), f.subject.as_str(), f.unit.as_str()))
        .collect();
    assert_eq!(
        binance_spot,
        vec![
            ("BNOUSDT", "coingecko:binanceonly", usdt.as_str()),
            ("EXCUSDT", "coingecko:examplecoin", usdt.as_str())
        ]
    );
    assert!(!b.report.contains("binanceonly: no crosswalked"));
    // A coin priced only in USDT gets a USD market: BNO/USDT × USDT/USD.
    let converted: Vec<(&str, &str, Option<&str>)> = b
        .snapshot
        .quote_derivations
        .iter()
        .map(|d| (d.subject.as_str(), d.method.as_str(), d.base.as_deref()))
        .collect();
    assert!(
        converted.contains(&("coingecko:binanceonly", "convert-via-stablecoin-v1", None)),
        "{converted:?}"
    );
    assert!(b.snapshot.quote_derivations.iter().all(|d| d.via == usdt));
    // S&P MidCap 400: a new equity (QQQ then reuses it), EXA reused by CUSIP.
    let mid: Vec<&str> = universe("sp400")
        .members
        .iter()
        .map(|m| m.node.as_str())
        .collect();
    assert_eq!(mid, vec!["spy:02079K305", "spy:037833100"]);
}
