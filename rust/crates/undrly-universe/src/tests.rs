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
        paths::NASDAQ100,
        br#"{"data":{"date":"Sep 24, 2026","data":{"rows":[
          {"symbol":"NVDA","companyName":"NVIDIA"},{"symbol":"EXA","companyName":"Example One"},
          {"symbol":"FRGN","companyName":"Foreign Co ADR"}]}}}"#,
    );
    put(
        paths::SEC_TICKERS,
        br#"{"fields":["cik","name","ticker","exchange"],"data":[
          [1045810,"NVIDIA CORP","NVDA","Nasdaq"],
          [111,"Example One Inc","EXA","Nasdaq"],
          [222,"Example plc","EXP","NYSE"],
          [333,"Example Holdings","EXB-B","NYSE"],
          [444,"OTC Co","OTCX","OTC"]]}"#,
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
    assert_eq!(b.snapshot.quote_aggregations.len(), 1);
    assert_eq!(
        b.snapshot.quote_aggregations[0].subject,
        "coingecko:examplecoin"
    );
    assert!(b.report.contains("GONE/USD"));
    assert!(
        b.report
            .contains("lookalike: no crosswalked Kraken or Coinbase USD market")
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
        vec!["EXAMPLE ONE INC", "EXAMPLE CL B", "EXAMPLE PLC"]
    );
    // Issuers carry their CIK; NVIDIA's V1 entity is reused.
    let ciks: Vec<&str> = b
        .snapshot
        .entities
        .iter()
        .map(|e| e.cik.as_deref().unwrap())
        .collect();
    assert_eq!(ciks, vec!["0000000111", "0000000222", "0000000333"]);
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
    assert_eq!(sp.as_of, "2026-09-23T00:00:00Z");
    assert!(
        b.report
            .contains("OTCX: SEC exchange Some(\"OTC\") has no MIC rule")
    );
    assert!(b.report.contains("is not a valid CUSIP"));
    // Nasdaq-100: members already imported and listed on Nasdaq.
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
    assert_eq!(symbols, vec!["EXA", "NVDA"]);
    assert!(
        b.report
            .contains("FRGN (Foreign Co ADR): not an imported S&P 500 security")
    );
}

#[test]
fn nasdaq_list_is_optional() {
    let mut files = files();
    files.remove(paths::NASDAQ100);
    let manifest = manifest(&files);
    let v1 = v1();
    let inputs = Inputs {
        manifest: &manifest,
        files: &files,
        v1: &v1,
        ids: IdMap::default(),
    };
    let b = build(&inputs, &mut counter()).unwrap();
    assert!(b.snapshot.universes.iter().all(|u| u.key != "nasdaq100"));
    assert!(b.report.contains("nasdaq100: not built"));
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
