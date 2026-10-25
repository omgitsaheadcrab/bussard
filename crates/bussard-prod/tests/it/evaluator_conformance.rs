//! The two evaluators of an application's Dynamic section agree (issue
//! #326): the model side (`bussard_model::device_view`, over the product
//! model's `blocks:` and `ref_values:`) and the download side
//! (`bussard_ets::dynamic::evaluate_dynamic`, over the application XML)
//! reach the same parameter refs for the same values. The model side
//! decides the plan's "ignored" notes and `knx_set_parameter`'s refusals,
//! the download side what an image holds; a disagreement is a false note or
//! a false refusal.
//!
//! Every fixture application in the repository is checked at the vendor
//! defaults and with each condition's tested value set on its own. With
//! `BUSSARD_METEO_SNAPSHOT` set to a model snapshot directory holding the
//! Theben Meteodata 1.1.202 (`knx/.bussard/history/20261023T190946Z-001`),
//! the device's own values are checked against its archive too (found at
//! `BUSSARD_METEO_KNXPROD`, else `<snapshot>/../../../products/`).
//!
//! The round trip (issue #345): the Dynamic evaluator over a program's
//! product model (format 5, which carries the tree) gives the same modules,
//! placements, containers and values as over its XML, on every fixture and,
//! with `BUSSARD_PRODUCT_CORPUS` set, on every program of the corpus.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use bussard_model::{IndividualAddress, Model, ProductModel, ProductModels};
use bussard_prod::product_model::write_product_models;
use bussard_prod::{ApplicationProgram, ProductData, parse_application_program};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// The fixture applications: (application id, XML).
const FIXTURES: &[(&str, &[u8])] = &[
    (
        "M-00FA_A-0298-10-0000",
        include_bytes!("../fixtures/blocks_app.xml"),
    ),
    (
        "A",
        include_bytes!("../../../bussard-download/tests/fixtures/param-readback/app.xml"),
    ),
    (
        "M-00FA_A-00D1-10-0001",
        include_bytes!("../../../bussard-project/tests/fixtures/dynamic_tree.app.xml"),
    ),
    (
        "M-00FA_A-00D2-10-0001",
        include_bytes!("../../../bussard-project/tests/fixtures/default_params.app.xml"),
    ),
    (
        "M-00FA_A-00D1-10-0001",
        include_bytes!("../../../bussard-ets/tests/fixtures/dynamic_tree.app.xml"),
    ),
];

/// The product model the importer writes for `app`.
fn product_model(app: &ApplicationProgram) -> Result<ProductModel, Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let product = ProductData {
        applications: vec![app.clone()],
        ..ProductData::default()
    };
    write_product_models(&product, dir.path())?;
    let path = dir
        .path()
        .join(".bussard/models")
        .join(format!("{}.yaml", app.id));
    let text = std::fs::read_to_string(&path)?;
    Ok(ProductModel::from_yaml(&text, &app.id)?)
}

/// The refs the download side reaches at the application level with
/// `values`, among `held` (the refs the model's pages hold).
fn download_reached(
    app: &ApplicationProgram,
    values: &BTreeMap<String, String>,
    held: &BTreeSet<String>,
) -> BTreeSet<String> {
    let config = bussard_prod::dynamic::evaluate_dynamic(app, values);
    config
        .parameters
        .iter()
        .zip(&config.parameter_placements)
        .filter(|(p, place)| p.module.is_none() && place.module.is_none() && place.shown)
        .map(|(p, _)| p.param_ref_id.clone())
        .filter(|r| held.contains(r))
        .collect()
}

/// The refs the model side shows for device `address` of `model`.
fn model_reached(
    model: &Model,
    products: &ProductModels,
    address: IndividualAddress,
) -> Result<BTreeSet<String>, Box<dyn std::error::Error>> {
    Ok(
        bussard_model::device_view::parameter_visibility(model, products, address)?
            .into_iter()
            .filter(|(_, v)| v.shown)
            .map(|(r, _)| r)
            .collect(),
    )
}

/// A one-device model running `app`, with `values` stored as values ETS
/// keeps outside the device file (the lock's `hidden[]`): the model side
/// reads them as it reads any stored value, and the flash writes them.
fn one_device(
    app: &str,
    values: &BTreeMap<String, String>,
) -> Result<Model, Box<dyn std::error::Error>> {
    let hidden: String = values
        .iter()
        .map(|(r, v)| format!("  {{ ref = \"{r}\", value = \"{v}\" }},\n"))
        .collect();
    let lock = format!(
        "version = 3\n\n[[device]]\naddress = \"1.1.1\"\nproduct = \"P-1\"\napplication = \
         \"{app}\"\nmask = \"0701\"\nhidden = [\n{hidden}]\n"
    );
    let files: BTreeMap<String, String> = [
        ("bussard.lock", lock),
        (
            "devices/1.1.1.toml",
            "address = \"1.1.1\"\nname = \"Device\"\nproduct = \"P-1\"\n".to_string(),
        ),
        ("groups.toml", "groups = []\n".to_string()),
        (
            "bussard.toml",
            "[connection]\ntransport = \"tunnel\"\n".to_string(),
        ),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    Ok(Model::from_texts(&files)?)
}

/// The vendor default of app-relative ref `reference` as the download side
/// takes it: the ref's `Value`, else the parameter's.
fn vendor_default(app: &ApplicationProgram, reference: &str) -> Option<f64> {
    let pref = app.parameter_ref(reference)?;
    let raw = pref.value.clone().or_else(|| {
        app.parameters
            .get(&pref.ref_id)
            .and_then(|p| p.default.clone())
    })?;
    raw.trim().parse().ok()
}

/// The value sets to compare under: none; each condition's tested value on
/// its own; and per ref the pages hold, the values that open the way to it
/// (every condition of its enclosing pages and of its branch), set only
/// where the vendor default does not already satisfy the condition, so a
/// condition the defaults hold (a ref value among them) is left to the
/// evaluators to resolve.
fn scenarios(app: &ApplicationProgram, product: &ProductModel) -> Vec<BTreeMap<String, String>> {
    let mut out = vec![BTreeMap::new()];
    let mut push = |values: BTreeMap<String, String>| {
        if !out.contains(&values) {
            out.push(values);
        }
    };
    for b in &product.blocks {
        for g in b.when.iter().chain(b.parameter_when.values().flatten()) {
            if let Some(v) = g.first_value() {
                push(BTreeMap::from([(g.reference.clone(), v.to_string())]));
            }
        }
    }
    let parents = bussard_model::blocks::parent_indexes(&product.blocks);
    for (i, b) in product.blocks.iter().enumerate() {
        let mut enclosing: Vec<&bussard_model::blocks::Gate> = Vec::new();
        let mut at = Some(i);
        while let Some(j) = at {
            enclosing.extend(product.blocks[j].when.iter());
            at = parents[j];
        }
        for reference in &b.parameters {
            let mut values = BTreeMap::new();
            let own = b.parameter_when.get(reference).into_iter().flatten();
            for g in enclosing.iter().copied().chain(own) {
                if g.holds(vendor_default(app, &g.reference)) {
                    continue;
                }
                if let Some(v) = g.first_value() {
                    values
                        .entry(g.reference.clone())
                        .or_insert_with(|| v.to_string());
                }
            }
            push(values);
        }
    }
    out
}

#[test]
fn test_model_and_download_evaluators_reach_the_same_refs_on_every_fixture() -> TestResult {
    let address: IndividualAddress = "1.1.1".parse()?;
    let mut compared = 0;
    for (id, xml) in FIXTURES {
        let app = parse_application_program(id, xml)?;
        let product = product_model(&app)?;
        if product.blocks.is_empty() {
            continue;
        }
        let held: BTreeSet<String> = product
            .blocks
            .iter()
            .flat_map(|b| b.parameters.iter().cloned())
            .collect();
        let mut products = ProductModels::default();
        products.insert(id.to_string(), product.clone());
        let scenarios = scenarios(&app, &product);
        compared += scenarios.len();
        for values in scenarios {
            let model = one_device(id, &values)?;
            let model_side = model_reached(&model, &products, address)?;
            let download_side = download_reached(&app, &values, &held);
            assert_eq!(
                model_side, download_side,
                "{id} with {values:?}: the model side and the download side reach different refs"
            );
        }
    }
    assert!(compared > 20, "only {compared} value sets compared");
    Ok(())
}

/// The snapshot directory and the Meteodata archive, when the corpus is at
/// hand.
fn meteo_corpus() -> Option<(PathBuf, PathBuf)> {
    let Some(snapshot) = std::env::var_os("BUSSARD_METEO_SNAPSHOT").map(PathBuf::from) else {
        eprintln!("skipping: BUSSARD_METEO_SNAPSHOT is not set");
        return None;
    };
    let archive = std::env::var_os("BUSSARD_METEO_KNXPROD")
        .map(PathBuf::from)
        .unwrap_or_else(|| snapshot.join("../../../products/M-0048_A-140C-14-EB70.knxprod"));
    if !snapshot.join("bussard.lock").is_file() || !archive.is_file() {
        eprintln!(
            "skipping: no snapshot at {} or no archive at {}",
            snapshot.display(),
            archive.display()
        );
        return None;
    }
    Some((snapshot, archive))
}

const METEO_APP: &str = "M-0048_A-140C-14-EB70";

/// The Meteodata 1.1.202 of the snapshot, its product model and its
/// application.
fn meteo(
    snapshot: &Path,
    archive: &Path,
) -> Result<(Model, ProductModels, ApplicationProgram), Box<dyn std::error::Error>> {
    let model = Model::load(snapshot)?;
    // The device file names enum members in the lock's language.
    let language = bussard_model::import_language(snapshot);
    let product =
        bussard_prod::read_knxprod_selected_in(archive, None, None, language.as_deref(), |_| {
            bussard_prod::AppSelection::Only(vec![METEO_APP.to_string()])
        })?;
    let app = product
        .application_by_id(METEO_APP)
        .ok_or("the archive carries no Meteodata application")?
        .clone();
    let mut products = ProductModels::default();
    products.insert(METEO_APP.to_string(), product_model(&app)?);
    Ok((model, products, app))
}

/// Corpus-gated (issue #326): on the owner's Meteodata the two evaluators
/// agree, so the plan carries no "ignored" note for the wind thresholds of
/// C1 the image writes, and every note it does carry names a value the
/// image leaves out.
#[test]
fn test_meteodata_snapshot_evaluators_agree_and_ignore_only_unwritten_values() -> TestResult {
    let Some((snapshot, archive)) = meteo_corpus() else {
        return Ok(());
    };
    let (model, products, app) = meteo(&snapshot, &archive)?;
    let address: IndividualAddress = "1.1.202".parse()?;
    let product = products.get(METEO_APP).ok_or("no product model")?;
    assert_eq!(
        product.ref_values.get("UP-2248_R-2262").map(String::as_str),
        Some("4")
    );
    let held: BTreeSet<String> = product
        .blocks
        .iter()
        .flat_map(|b| b.parameters.iter().cloned())
        .collect();
    let (values, malformed) = bussard_download_overrides(&model, address);
    assert!(malformed.is_empty(), "{malformed:?}");
    let model_side = model_reached(&model, &products, address)?;
    let download_side = download_reached(&app, &values, &held);
    assert!(model_side.contains("P-613_R-2263"), "C1 wind threshold");
    assert_eq!(model_side, download_side);
    // Every value the plan calls ignored is one the image does not hold:
    // its ref is unreached on the download side too. None of C1's wind
    // values is among them. The two that are: ETS keeps a value for the C18
    // logic inputs' XOR refs (`@P-6823_R-6823`, `@P-6824_R-6824`), while the
    // channel runs UND and shows the inputs through other refs of the same
    // parameters, which the image writes at their defaults.
    let ignored = bussard_model::device_view::ignored_overrides(&model, &products, address)?;
    let refs: BTreeSet<&str> = ignored.iter().map(|o| o.reference.as_str()).collect();
    for r in &refs {
        assert!(
            !download_side.contains(*r),
            "{r} is written but called ignored"
        );
    }
    assert_eq!(
        refs,
        BTreeSet::from(["P-6823_R-6823", "P-6824_R-6824"]),
        "{ignored:?}"
    );
    Ok(())
}

/// The device's values keyed by ref, as the flash reads them (the part of
/// each in-memory key after `@`).
fn bussard_download_overrides(
    model: &Model,
    address: IndividualAddress,
) -> (BTreeMap<String, String>, Vec<String>) {
    let mut out = BTreeMap::new();
    let mut malformed = Vec::new();
    if let Some(d) = model.devices.get(&address) {
        for (key, value) in &d.device.parameters {
            match key.split_once('@') {
                Some((_, r)) if !r.is_empty() => {
                    out.insert(r.to_string(), value.clone());
                }
                _ => malformed.push(key.clone()),
            }
        }
    }
    (out, malformed)
}

// ---------------------------------------------------------------------------
// The lossless round trip (issue #345): the product model carries the
// Dynamic tree, so the evaluator gives the same answer over the model as
// over the application XML.
// ---------------------------------------------------------------------------

/// A small seeded generator (SplitMix64) for the random value sets.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number below `n` (0 for `n` 0).
    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }

    /// True once in `n` draws.
    fn one_in(&mut self, n: u64) -> bool {
        self.below(n) == 0
    }
}

/// How many random value sets each application is compared under, besides
/// the vendor defaults.
const RANDOM_SETS: u64 = 50;

/// One parameter ref a value set may set: its device-file keys (the ref
/// itself, or one per module instance with the selector), the values its
/// type allows and the values its `<choose>` tests name.
struct Candidate {
    keys: Vec<String>,
    codes: Vec<i64>,
    range: Option<(i64, i64)>,
    tested: Vec<i64>,
    controls: bool,
}

/// The values each `<choose>` ref's tests name, and every module instance
/// with its definition.
type TreeFacts = (BTreeMap<String, BTreeSet<i64>>, Vec<(String, String)>);

/// Every `<choose>` ref with the values its tests name (and their
/// neighbours, for the comparisons), every `<Assign>` source, and every
/// module instance with its definition.
fn tree_facts(app: &ApplicationProgram) -> TreeFacts {
    use bussard_prod::dynamic::{DynamicNode, WhenTest};
    fn visit(
        nodes: &[DynamicNode],
        tested: &mut BTreeMap<String, BTreeSet<i64>>,
        modules: &mut Vec<(String, String)>,
    ) {
        for node in nodes {
            match node {
                DynamicNode::Choose {
                    param_ref_id,
                    whens,
                } => {
                    let set = tested.entry(param_ref_id.clone()).or_default();
                    for w in whens {
                        match &w.test {
                            WhenTest::Values(vs) => set.extend(vs.iter().copied()),
                            WhenTest::Compare { value, .. } => set.extend([
                                value.saturating_sub(1),
                                *value,
                                value.saturating_add(1),
                            ]),
                            WhenTest::Default | WhenTest::Unknown(_) => {}
                        }
                    }
                    for w in whens {
                        visit(&w.children, tested, modules);
                    }
                }
                DynamicNode::Channel { children, .. }
                | DynamicNode::ParameterBlock { children, .. } => visit(children, tested, modules),
                DynamicNode::Module { id, module_def, .. } => {
                    modules.push((id.clone(), module_def.clone()))
                }
                DynamicNode::Assign {
                    source: Some(source),
                    ..
                } => {
                    tested.entry(source.clone()).or_default();
                }
                DynamicNode::ParameterRefRef(_)
                | DynamicNode::ComObjectRefRef(_)
                | DynamicNode::Assign { .. } => {}
            }
        }
    }
    let mut tested = BTreeMap::new();
    let mut modules = Vec::new();
    visit(&app.dynamic, &mut tested, &mut modules);
    let mut defs: Vec<&String> = app.module_dynamics.keys().collect();
    defs.sort();
    for def in defs {
        if let Some(body) = app.module_dynamics.get(def) {
            visit(body, &mut tested, &mut modules);
        }
    }
    modules.sort();
    modules.dedup();
    (tested, modules)
}

/// The refs a random value set draws from.
fn candidates(app: &ApplicationProgram) -> Vec<Candidate> {
    let (tested, modules) = tree_facts(app);
    let prefix = format!("{}_", app.id);
    let mut out = Vec::new();
    let mut refs: Vec<&bussard_prod::ParameterRef> = app.parameter_refs.values().collect();
    refs.sort_by(|a, b| a.id.cmp(&b.id));
    for pref in refs {
        let Some(rel) = pref.id.strip_prefix(&prefix) else {
            continue;
        };
        let Some(kind) = app
            .parameters
            .get(&pref.ref_id)
            .and_then(|p| p.parameter_type.as_deref())
            .and_then(|t| app.parameter_types.get(t))
            .map(|t| &t.kind)
        else {
            continue;
        };
        let (codes, range) = match kind {
            bussard_prod::ParameterType::Enum { values, .. } => {
                (values.iter().map(|v| v.value).collect(), None)
            }
            bussard_prod::ParameterType::Int { min, max, .. } => {
                let lo = min.unwrap_or(0);
                (
                    Vec::new(),
                    Some((lo, max.unwrap_or(lo.saturating_add(255)))),
                )
            }
            _ => continue,
        };
        let keys: Vec<String> = match rel.split_once('_') {
            Some((md, _)) if md.starts_with("MD-") => modules
                .iter()
                .filter(|(_, def)| def == md)
                .map(|(instance, _)| bussard_prod::dynamic::selector_key(Some(instance), rel))
                .collect(),
            _ => vec![rel.to_string()],
        };
        if keys.is_empty() {
            continue;
        }
        let (controls, tested) = match tested.get(rel) {
            Some(values) => (true, values.iter().copied().collect()),
            None => (false, Vec::new()),
        };
        out.push(Candidate {
            keys,
            codes,
            range,
            tested,
            controls,
        });
    }
    out
}

/// One random valid value set: each controlling ref (a `<choose>` or an
/// `<Assign>` source) in each of its scopes with a chance of one in two,
/// any other ref with one in sixteen; the value one of the enumeration's
/// codes or within the integer range, a tested value half of the time.
fn random_values(candidates: &[Candidate], rng: &mut Rng) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for c in candidates {
        for key in &c.keys {
            if !rng.one_in(if c.controls { 2 } else { 16 }) {
                continue;
            }
            let allowed = |v: &i64| match c.range {
                Some((lo, hi)) => (lo..=hi).contains(v),
                None => c.codes.contains(v),
            };
            let tested: Vec<i64> = c.tested.iter().copied().filter(allowed).collect();
            let value = if !tested.is_empty() && rng.one_in(2) {
                tested[rng.below(tested.len() as u64) as usize]
            } else if let Some((lo, hi)) = c.range {
                let span = hi.saturating_sub(lo).max(0) as u64;
                lo.saturating_add(rng.below(span.saturating_add(1)) as i64)
            } else if c.codes.is_empty() {
                continue;
            } else {
                c.codes[rng.below(c.codes.len() as u64) as usize]
            };
            out.insert(key.clone(), value.to_string());
        }
    }
    out
}

/// The first index at which two lists differ, as a message.
fn first_difference<T: PartialEq + std::fmt::Debug>(
    what: &str,
    a: &[T],
    b: &[T],
) -> Option<String> {
    if let Some(i) = (0..a.len().min(b.len())).find(|&i| a[i] != b[i]) {
        return Some(format!("{what}[{i}]: XML {:?}, model {:?}", a[i], b[i]));
    }
    (a.len() != b.len()).then(|| format!("{what}: XML has {}, model {}", a.len(), b.len()))
}

/// `None` when the evaluator gives the same answer over `app` and over
/// `model` with `values`, else the first difference.
fn round_trip_difference(
    app: &ApplicationProgram,
    model: &ProductModel,
    values: &BTreeMap<String, String>,
) -> Result<bussard_model::dynamic::DynamicConfig, String> {
    use bussard_model::dynamic::evaluate_dynamic;
    let xml = evaluate_dynamic(app, values);
    let yaml = evaluate_dynamic(model, values);
    let found = first_difference("modules", &xml.modules, &yaml.modules)
        .or_else(|| first_difference("parameters", &xml.parameters, &yaml.parameters))
        .or_else(|| {
            first_difference(
                "parameter_placements",
                &xml.parameter_placements,
                &yaml.parameter_placements,
            )
        })
        .or_else(|| first_difference("com_objects", &xml.com_objects, &yaml.com_objects))
        .or_else(|| {
            first_difference(
                "com_object_placements",
                &xml.com_object_placements,
                &yaml.com_object_placements,
            )
        })
        .or_else(|| first_difference("containers", &xml.containers, &yaml.containers))
        .or_else(|| {
            first_difference(
                "unresolved_overrides",
                &xml.unresolved_overrides,
                &yaml.unresolved_overrides,
            )
        });
    if let Some(found) = found {
        return Err(found);
    }
    let value = xml.parameters.iter().find_map(|p| {
        let r = p.param_ref_id.as_str();
        let a = (
            xml.value(app, p.module, r),
            xml.vendor_default(app, p.module, r),
            xml.is_assigned(p.module, r),
        );
        let b = (
            yaml.value(model, p.module, r),
            yaml.vendor_default(model, p.module, r),
            yaml.is_assigned(p.module, r),
        );
        (a != b).then(|| format!("value of {r} in {:?}: XML {a:?}, model {b:?}", p.module))
    });
    match value {
        Some(found) => Err(found),
        None => Ok(xml),
    }
}

/// Compares the evaluator over `app` and over its product model under the
/// vendor defaults and [`RANDOM_SETS`] random value sets seeded by the
/// application id. Returns how many sets were compared and how many reached
/// a configuration other than the defaults'.
fn check_round_trip(
    app: &ApplicationProgram,
    product: &ProductData,
) -> Result<(usize, usize), String> {
    let yaml = bussard_prod::product_model::model_yaml(app, product).map_err(|e| e.to_string())?;
    let model = ProductModel::from_yaml(&yaml, &app.id).map_err(|e| e.to_string())?;
    if bussard_model::dynamic::RefIndex::new(app) != bussard_model::dynamic::RefIndex::new(&model) {
        return Err("the ref index differs".to_string());
    }
    let defaults = BTreeMap::new();
    let base = round_trip_difference(app, &model, &defaults)
        .map_err(|diff| format!("vendor defaults: {diff}"))?
        .parameters;
    let candidates = candidates(app);
    let seed = app
        .id
        .bytes()
        .fold(0xC0FF_EE00_u64, |h, b| h.rotate_left(5) ^ u64::from(b));
    let mut rng = Rng(seed);
    let mut other = 0;
    for set in 0..RANDOM_SETS {
        let values = random_values(&candidates, &mut rng);
        let config = round_trip_difference(app, &model, &values)
            .map_err(|diff| format!("set {set} ({} values): {diff}", values.len()))?;
        if config.parameters != base {
            other += 1;
        }
    }
    Ok((RANDOM_SETS as usize + 1, other))
}

#[test]
fn test_product_model_round_trips_the_dynamic_tree_on_every_fixture() -> TestResult {
    let mut compared = 0;
    let mut other = 0;
    for (id, xml) in FIXTURES {
        let app = parse_application_program(id, xml)?;
        let product = ProductData {
            applications: vec![app.clone()],
            ..ProductData::default()
        };
        let (sets, moved) = check_round_trip(&app, &product).map_err(|e| format!("{id}: {e}"))?;
        compared += sets;
        other += moved;
    }
    assert_eq!(compared, FIXTURES.len() * (RANDOM_SETS as usize + 1));
    assert!(
        other > 50,
        "only {other} random sets reached another configuration than the defaults"
    );
    Ok(())
}

/// The `.knxprod` archives at or one level under `dir`, sorted.
fn knxprod_files(dir: &Path) -> Vec<PathBuf> {
    let is_knxprod = |p: &Path| p.extension().and_then(|e| e.to_str()) == Some("knxprod");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            for sub in std::fs::read_dir(&path).into_iter().flatten().flatten() {
                if is_knxprod(&sub.path()) {
                    out.push(sub.path());
                }
            }
        } else if is_knxprod(&path) {
            out.push(path);
        }
    }
    out.sort();
    out
}

/// Corpus-gated (`BUSSARD_PRODUCT_CORPUS`, docs/testing.md): the round trip
/// over every application program of every archive in the corpus.
#[test]
fn test_product_model_round_trips_the_dynamic_tree_on_the_corpus() -> TestResult {
    let Some(dir) = std::env::var_os("BUSSARD_PRODUCT_CORPUS").map(PathBuf::from) else {
        eprintln!("BUSSARD_PRODUCT_CORPUS unset; skipping the Dynamic round trip sweep.");
        return Ok(());
    };
    let vendor = [dir.join("cache/vendor"), dir.join("vendor"), dir]
        .into_iter()
        .find(|d| !knxprod_files(d).is_empty())
        .ok_or("the corpus holds no .knxprod files")?;
    let files = knxprod_files(&vendor);
    let started = std::time::Instant::now();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new(Vec::new());
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get().clamp(2, 10));
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(file) = files.get(i) else { break };
                    let name = file.file_name().map(|n| n.to_string_lossy().into_owned());
                    let name = name.unwrap_or_default();
                    let outcome = match bussard_prod::read_knxprod(file) {
                        Err(_) => vec![(name, None, Ok((0, 0)))],
                        Ok(product) => product
                            .applications
                            .iter()
                            .map(|app| {
                                (
                                    name.clone(),
                                    Some(app.id.clone()),
                                    check_round_trip(app, &product),
                                )
                            })
                            .collect(),
                    };
                    if let Ok(mut all) = results.lock() {
                        all.extend(outcome);
                    }
                }
            });
        }
    });
    let results = results
        .into_inner()
        .map_err(|_| "a sweep thread panicked")?;
    let mut programs = 0;
    let mut sets = 0;
    let mut other = 0;
    let mut unreadable = 0;
    let mut failures = Vec::new();
    for (archive, app, outcome) in results {
        match (app, outcome) {
            (None, _) => unreadable += 1,
            (Some(app), Ok((n, moved))) => {
                programs += 1;
                sets += n;
                other += moved;
                let _ = app;
            }
            (Some(app), Err(e)) => failures.push(format!("{archive} {app}: {e}")),
        }
    }
    eprintln!(
        "round trip: {} archives ({unreadable} unreadable), {programs} programs, {sets} value \
         sets ({other} off the defaults), {} failed, {:.0?}",
        files.len(),
        failures.len(),
        started.elapsed()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(programs > 0, "no program compared");
    Ok(())
}
