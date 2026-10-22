//! `bussard device` / `knx_show_device` on an application without channels
//! of its own (issue #298), and the capability lookup over the product
//! model's parameter and choice texts.
//!
//! The product model is `fixtures/blocks_app.yaml`, written by the importer
//! from the fabricated System 7 application in
//! `crates/bussard-prod/tests/fixtures/blocks_app.xml`: universal channels A,
//! B and C are ETS pages behind "enable" parameters (C behind B), each with a
//! "Channel function" whose first choice is a brightness threshold. The
//! device enables A with the temperature function; B and C are spare.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bussard_model::capabilities::{cache_path, capability_lookup};
use bussard_model::device_view::device_view;
use bussard_model::{IndividualAddress, Model, ProductModel, ProductModels};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const APP: &str = "M-00FA_A-0298-10-0000";

const PRODUCT: &str = include_str!("fixtures/blocks_app.yaml");

const LOCK: &str = r#"version = 3

[[device]]
address = "1.1.30"
product = "WS-298"
application = "M-00FA_A-0298-10-0000"
mask = "0701"
channels = [
  { key = "generic-0", id = "CH-0", number = 0 },
]
objects = [
  { number = 0, key = "brightness", channel = "generic-0", text = "Brightness", function = "Physical value", dpt = "9.004", flags = "CRT", ref = "O-0_R-0" },
]
parameters = [
  { key = "universal-channel-a-enable", channel = "generic-0", ref = "P-1_R-1", param = "P-1" },
  { key = "universal-channel-b-enable", channel = "generic-0", ref = "P-2_R-2", param = "P-2" },
  { key = "channel-function", channel = "generic-0", ref = "P-11_R-11", param = "P-11" },
]
"#;

const DEVICE: &str = r#"address = "1.1.30"
name = "Weather station"
product = "WS-298"

[channel.generic-0]
universal-channel-a-enable = "yes"
channel-function = "Temperature sensor"
brightness.send = "4/1/5"
"#;

const GROUPS: &str =
    "groups = [\n  { address = \"4/1/5\", name = \"Brightness\", dpt = \"9.004\" },\n]\n";

fn files() -> [(&'static str, &'static str); 4] {
    [
        ("bussard.lock", LOCK),
        ("devices/1.1.30.toml", DEVICE),
        ("groups.toml", GROUPS),
        ("bussard.toml", "[connection]\ntransport = \"tunnel\"\n"),
    ]
}

fn model() -> Result<Model, Box<dyn std::error::Error>> {
    let files: BTreeMap<String, String> = files()
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    Ok(Model::from_texts(&files)?)
}

fn products() -> Result<ProductModels, Box<dyn std::error::Error>> {
    let mut products = ProductModels::default();
    products
        .by_app_ref
        .insert(APP.to_string(), ProductModel::from_yaml(PRODUCT, APP)?);
    Ok(products)
}

fn ia() -> Result<IndividualAddress, Box<dyn std::error::Error>> {
    Ok("1.1.30".parse()?)
}

/// A scratch model directory with the files and the product model, removed
/// on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let dir = std::env::temp_dir().join(format!(
            "bussard-device-blocks-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("devices"))?;
        std::fs::create_dir_all(dir.join(".bussard/models"))?;
        for (name, text) in files() {
            std::fs::write(dir.join(name), text)?;
        }
        std::fs::write(dir.join(format!(".bussard/models/{APP}.yaml")), PRODUCT)?;
        Ok(Scratch(dir))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn test_device_view_groups_a_channel_less_app_by_page() -> TestResult {
    let view = device_view(&model()?, &products()?, ia()?, None)?;
    let rows: Vec<(&str, bool)> = view
        .blocks
        .iter()
        .map(|b| (b.handle.as_str(), b.active))
        .collect();
    // B's objects page is folded into B (the same switch shows it), and C
    // sits behind B's switch and its own.
    assert_eq!(
        rows,
        [
            ("general", true),
            ("universal-channel-a-function", true),
            ("universal-channel-b-function", false),
            ("universal-channel-c-function", false),
        ]
    );
    let general = &view.blocks[0];
    assert_eq!((general.parameters, general.objects), (2, 0));
    let a = &view.blocks[1];
    assert_eq!(a.parameters, 1);
    assert!(a.inactive.is_none());

    let b = &view.blocks[2];
    assert_eq!(b.includes, ["PB-21"]);
    let inactive = b.inactive.as_ref().ok_or("B is inactive")?;
    assert_eq!(inactive.enable.len(), 1);
    let step = &inactive.enable[0];
    assert_eq!(step.key.as_deref(), Some("universal-channel-b-enable"));
    assert_eq!(step.set_to, "yes");
    assert_eq!(step.default.as_deref(), Some("no"));
    let shown: Vec<(u16, Option<&str>)> = inactive
        .would_show
        .iter()
        .map(|o| (o.number, o.function.as_deref()))
        .collect();
    assert_eq!(shown, [(2, Some("Switch"))]);
    assert!(
        inactive.needs.contains("bussard flash 1.1.30"),
        "{}",
        inactive.needs
    );

    // C needs B first, then its own switch (a parameter the configuration
    // does not show yet, so no key).
    let c = view.blocks[3].inactive.as_ref().ok_or("C is inactive")?;
    let steps: Vec<(Option<&str>, &str)> = c
        .enable
        .iter()
        .map(|e| (e.key.as_deref(), e.set_to.as_str()))
        .collect();
    assert_eq!(
        steps,
        [(Some("universal-channel-b-enable"), "yes"), (None, "yes")]
    );
    assert_eq!(
        c.enable[1].text.as_deref(),
        Some("Universal channel C enable")
    );

    let summary = view.summary.as_deref().ok_or("a summary")?;
    assert_eq!(
        summary,
        "2 of 4 pages inactive: 2 Universal channel B: function .. Universal channel C: function"
    );

    let text = view.render_table();
    assert!(
        text.contains("pages (ETS parameter blocks, in ETS order):"),
        "{text}"
    );
    assert!(
        text.contains(
            "inactive: set universal-channel-b-enable = \"yes\" (default \"no\"); shows 1 object(s)"
        ),
        "{text}"
    );
    assert!(text.contains(&format!("summary: {summary}")), "{text}");
    Ok(())
}

#[test]
fn test_device_view_details_an_inactive_page() -> TestResult {
    let view = device_view(
        &model()?,
        &products()?,
        ia()?,
        Some("universal-channel-b-function"),
    )?;
    let scope = view.scope.as_ref().ok_or("no scope")?;
    assert_eq!(scope.block.as_deref(), Some("universal-channel-b-function"));
    assert!(scope.inactive.is_some());
    let params: Vec<(&str, Option<&str>)> = scope
        .parameters
        .iter()
        .map(|p| (p.key.as_str(), p.default.as_deref()))
        .collect();
    assert_eq!(
        params,
        [
            (
                "channel-function",
                Some("Brightness sensor 1 .. 100 000 lx")
            ),
            ("dusk-threshold-in-lx", Some("10")),
        ]
    );
    let objects: Vec<u16> = scope.objects.iter().map(|o| o.number).collect();
    assert_eq!(objects, [2]);

    // An active page lists the device's own parameters on it.
    let view = device_view(
        &model()?,
        &products()?,
        ia()?,
        Some("universal-channel-a-function"),
    )?;
    let scope = view.scope.as_ref().ok_or("no scope")?;
    assert!(scope.inactive.is_none());
    let keys: Vec<(&str, Option<&str>)> = scope
        .parameters
        .iter()
        .map(|p| (p.key.as_str(), p.value.as_deref()))
        .collect();
    assert_eq!(keys, [("channel-function", Some("Temperature sensor"))]);
    Ok(())
}

#[test]
fn test_capability_lookup_finds_a_choice_on_an_inactive_page() -> TestResult {
    let scratch = Scratch::new("lookup")?;
    let model = Model::load(scratch.path())?;
    let hits = capability_lookup(&model, scratch.path(), "brightness sensor", 50);
    let got: Vec<(Option<&str>, bool, bool, &str)> = hits
        .iter()
        .map(|h| {
            (
                h.block_or_channel.as_deref(),
                h.active,
                h.selected,
                h.status.as_str(),
            )
        })
        .collect();
    // Spare capacity (B and C, inactive) before repurposing A, which is
    // active with another function.
    assert_eq!(
        got,
        [
            (
                Some("Universal channel B: function"),
                false,
                false,
                "could provide"
            ),
            (
                Some("Universal channel C: function"),
                false,
                false,
                "could provide"
            ),
            (
                Some("Universal channel A: function"),
                true,
                false,
                "could provide"
            ),
        ]
    );
    let b = &hits[0];
    assert_eq!(b.device_ia, "1.1.30");
    assert_eq!(b.parameter, "Channel function");
    assert_eq!(
        b.choice.as_deref(),
        Some("Brightness sensor 1 .. 100 000 lx")
    );
    assert_eq!(b.handle.as_deref(), Some("universal-channel-b-function"));
    assert_eq!(
        b.enable.first().and_then(|e| e.key.as_deref()),
        Some("universal-channel-b-enable")
    );
    assert!(
        b.needs
            .as_deref()
            .is_some_and(|n| n.contains("bussard flash"))
    );

    // A parameter text matches too, and an active one the device shows
    // provides it.
    let hits = capability_lookup(&model, scratch.path(), "temperature", 50);
    assert_eq!(hits.first().map(|h| h.status.as_str()), Some("provides"));
    assert_eq!(
        hits.first().and_then(|h| h.key.as_deref()),
        Some("channel-function")
    );

    // Case does not matter. A's dusk threshold is on an active page but
    // hidden by its function, so it could provide, not provides.
    let hits = capability_lookup(&model, scratch.path(), "DUSK threshold", 50);
    let got: Vec<(Option<&str>, &str)> = hits
        .iter()
        .map(|h| (h.block_or_channel.as_deref(), h.status.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            (Some("Universal channel B: function"), "could provide"),
            (Some("Universal channel A: function"), "could provide"),
        ]
    );
    Ok(())
}

#[test]
fn test_capability_lookup_caches_the_model_and_rebuilds_a_stale_cache() -> TestResult {
    let scratch = Scratch::new("cache")?;
    let model = Model::load(scratch.path())?;
    let cache = cache_path(scratch.path(), APP);
    assert!(!cache.exists());
    let first = capability_lookup(&model, scratch.path(), "brightness", 50);
    assert!(cache.is_file(), "the lookup caches the parsed model");

    // A cache read gives the same answer.
    let again = capability_lookup(&model, scratch.path(), "brightness", 50);
    assert_eq!(first, again);

    // A model file newer than the cache is parsed again: rename a choice.
    std::thread::sleep(std::time::Duration::from_millis(20));
    let renamed = PRODUCT.replace("Wind sensor", "Anemometer");
    let yaml = scratch.path().join(format!(".bussard/models/{APP}.yaml"));
    std::fs::write(&yaml, renamed)?;
    let future = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    std::fs::File::options()
        .write(true)
        .open(&yaml)?
        .set_modified(future)?;
    let hits = capability_lookup(&model, scratch.path(), "anemometer", 50);
    assert_eq!(hits.len(), 3, "{hits:?}");
    Ok(())
}
