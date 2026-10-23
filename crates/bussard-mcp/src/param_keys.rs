//! Parameter keys as `knx_set_parameter` takes them (issue #304).
//!
//! `knx_show_device` shows a parameter under the key the lock assigns: the
//! slug of its text (`helligkeitsschwelle`), page-qualified when the text
//! repeats (`sollwerte.komfort`), or `<slug>@<ref>` as the escape hatch. The
//! resolver takes each of those, the key prefixed with the page or channel
//! handle it sits on (`universal-channel-a-function.channel-function`), and
//! the bare `ParameterRef` id (`P-11_R-11`). `channel` takes a channel
//! handle, `device`, or a page handle, exactly as `knx_show_device` does.
//! When nothing matches, the refusal names the three closest keys on the
//! device with the page or channel each sits on.

use std::collections::{BTreeMap, HashMap};

use bussard_model::blocks::ref_param_id;
use bussard_model::device_view::{DEVICE_SCOPE, parameter_pages};
use bussard_model::param_model::ProductModels;
use bussard_model::schema::Device;
use bussard_model::{IndividualAddress, Model, param_mem_key, slug};

/// The key grammar, stated once for the tool descriptions, `how_to_change`
/// and the refusals.
pub const KEY_GRAMMAR: &str = "Parameter keys: the key knx_show_device lists \
     (`helligkeitsschwelle`, page-qualified `sollwerte.komfort`, or `key@reference`), that key \
     prefixed with its page or channel handle (`universalkanal-c4-funktion.helligkeitsschwelle`), \
     or the bare reference (`P-7305_R-7305`); `channel` takes a channel handle, `device` or a \
     page handle";

/// What the resolver knows about a device beyond its lock: the ETS page each
/// parameter ref sits on and the vendor text of each.
#[derive(Debug, Default)]
pub struct KeyContext {
    /// Page handle by app-relative `ParameterRef` id.
    pages: BTreeMap<String, String>,
    /// Vendor text by app-relative `ParameterRef` id.
    texts: HashMap<String, String>,
}

impl KeyContext {
    /// The context of device `address` in `model`, from the product models
    /// in `products` (empty without one).
    pub fn new(model: &Model, products: &ProductModels, address: IndividualAddress) -> Self {
        let pages = parameter_pages(model, products, address).unwrap_or_default();
        let mut texts = HashMap::new();
        if let Some(loaded) = model.devices.get(&address) {
            let device = &loaded.device;
            let product = device
                .product
                .as_ref()
                .and_then(|p| p.application_ref.as_deref())
                .and_then(|a| products.get(a));
            for (reference, p) in &device.lock.parameters {
                let id = p
                    .param
                    .as_deref()
                    .unwrap_or_else(|| ref_param_id(reference));
                let text = product
                    .and_then(|m| m.parameters.get(id))
                    .and_then(|d| d.text.clone().or_else(|| d.name.clone()));
                if let Some(text) = text {
                    texts.insert(reference.clone(), text);
                }
            }
        }
        KeyContext { pages, texts }
    }
}

/// The last part of a page-qualified key, without an `@reference` suffix.
fn last_part(key: &str) -> &str {
    let head = key.split_once('@').map_or(key, |(h, _)| h);
    head.rsplit_once('.').map_or(head, |(_, t)| t)
}

/// The ref of an in-memory or escape-hatch key (`<slug>@<ref>`).
fn ref_of(key: &str) -> Option<&str> {
    key.split_once('@')
        .map(|(_, r)| r)
        .filter(|r| !r.is_empty())
}

/// The handle of the channel parameter ref `reference` sits in, or `device`.
fn channel_of(device: &Device, reference: &str) -> String {
    device
        .lock
        .parameters
        .get(reference)
        .and_then(|p| p.channel.as_deref())
        .map(|id| device.channel_handle(id))
        .unwrap_or_else(|| DEVICE_SCOPE.to_string())
}

/// The page or channel handle `reference` is listed under: its page when
/// the device lists pages, else its channel (or `device`).
fn place_of(device: &Device, ctx: &KeyContext, reference: &str) -> String {
    ctx.pages
        .get(reference)
        .cloned()
        .unwrap_or_else(|| channel_of(device, reference))
}

/// Whether `scope` (a channel handle or id, `device`, or a page handle)
/// holds parameter ref `reference`.
fn in_scope(device: &Device, ctx: &KeyContext, reference: &str, scope: &str) -> bool {
    if ctx.pages.get(reference).is_some_and(|h| h == scope) {
        return true;
    }
    let channel = device
        .lock
        .parameters
        .get(reference)
        .and_then(|p| p.channel.as_deref());
    match channel {
        Some(id) => id == scope || device.channel_handle(id) == scope,
        None => scope == DEVICE_SCOPE,
    }
}

/// Whether `scope` names a channel or page of the device at all.
fn known_scope(device: &Device, ctx: &KeyContext, scope: &str) -> bool {
    scope == DEVICE_SCOPE
        || ctx.pages.values().any(|h| h == scope)
        || device
            .channels
            .keys()
            .any(|id| id == scope || device.channel_handle(id) == scope)
}

/// The refs the lock lists for `wanted` within every scope of `scopes`.
fn find<'a>(device: &'a Device, ctx: &KeyContext, wanted: &str, scopes: &[&str]) -> Vec<&'a str> {
    let params = &device.lock.parameters;
    let fits = |r: &str| scopes.iter().all(|s| in_scope(device, ctx, r, s));
    // `key@reference` and the bare reference name one ref.
    let by_ref = wanted.rsplit_once('@').map_or(wanted, |(_, r)| r);
    if let Some((reference, _)) = params.get_key_value(by_ref) {
        return if fits(reference) {
            vec![reference.as_str()]
        } else {
            Vec::new()
        };
    }
    let exact: Vec<&str> = params
        .iter()
        .filter(|(r, p)| p.key == wanted && fits(r))
        .map(|(r, _)| r.as_str())
        .collect();
    if !exact.is_empty() {
        return exact;
    }
    // `block.key`: the key on the page or channel `block`.
    if let Some((head, tail)) = wanted.split_once('.')
        && known_scope(device, ctx, head)
    {
        let mut inner = scopes.to_vec();
        inner.push(head);
        let hits = find(device, ctx, tail, &inner);
        if !hits.is_empty() {
            return hits;
        }
    }
    // A page-qualified key by its last part, as the loader reads it.
    let tail = last_part(wanted);
    params
        .iter()
        .filter(|(r, p)| last_part(&p.key) == tail && fits(r))
        .map(|(r, _)| r.as_str())
        .collect()
}

/// The in-memory key (`<slug>@<ref>`) of the parameter a caller names
/// `wanted` on `device`, in `channel` when given (see [`KEY_GRAMMAR`]).
///
/// # Errors
///
/// A sentence for the caller when `channel` names no channel or page, when
/// nothing matches (naming the three closest keys), or when `wanted` names
/// a parameter in several places.
pub fn resolve_parameter_key(
    device: &Device,
    ctx: &KeyContext,
    wanted: &str,
    channel: Option<&str>,
) -> Result<String, String> {
    let wanted = wanted.trim();
    if device.parameters.contains_key(wanted) {
        return Ok(wanted.to_string());
    }
    let channel = channel.map(str::trim).filter(|c| !c.is_empty());
    if let Some(scope) = channel
        && !known_scope(device, ctx, scope)
    {
        let mut known: Vec<String> = device
            .channels
            .keys()
            .map(|id| device.channel_handle(id))
            .collect();
        known.push(DEVICE_SCOPE.to_string());
        let mut pages: Vec<String> = ctx.pages.values().cloned().collect();
        pages.sort_unstable();
        pages.dedup();
        known.extend(pages);
        return Err(format!(
            "{} has no channel or page {scope:?}; its channels and pages are: {}",
            device.address,
            known.join(", ")
        ));
    }
    // A stored key whose ref is named (`key@reference`) and that the lock
    // does not list.
    let reference = ref_of(wanted).unwrap_or(wanted);
    if !device.lock.parameters.contains_key(reference)
        && let Some(key) = device
            .parameters
            .keys()
            .find(|k| ref_of(k) == Some(reference))
    {
        return Ok(key.clone());
    }
    let scopes: Vec<&str> = channel.into_iter().collect();
    let mut hits = find(device, ctx, wanted, &scopes);
    hits.sort_unstable();
    hits.dedup();
    match hits.as_slice() {
        [reference] => Ok(mem_key(device, reference)),
        [] => Err(no_match(device, ctx, wanted, channel)),
        many => Err(format!(
            "{wanted:?} names a parameter in several places on {}: {}. Use one of these keys, or \
             pass `channel`",
            device.address,
            many.iter()
                .map(|r| format!("`{}`", qualified(device, ctx, r)))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// The key `reference` is stored under, else the one the loader derives.
fn mem_key(device: &Device, reference: &str) -> String {
    device
        .parameters
        .keys()
        .find(|k| ref_of(k) == Some(reference))
        .cloned()
        .unwrap_or_else(|| {
            let key = device
                .lock
                .parameters
                .get(reference)
                .map_or(reference, |p| p.key.as_str());
            param_mem_key(key, reference)
        })
}

/// `<place>.<key>` for `reference`: a key that names it on its own.
fn qualified(device: &Device, ctx: &KeyContext, reference: &str) -> String {
    let key = device
        .lock
        .parameters
        .get(reference)
        .map_or(reference, |p| p.key.as_str());
    format!("{}.{key}", place_of(device, ctx, reference))
}

/// The refusal for a key that matches nothing, with the three closest keys.
fn no_match(device: &Device, ctx: &KeyContext, wanted: &str, channel: Option<&str>) -> String {
    let within = channel.map(|c| format!(" in {c}")).unwrap_or_default();
    let close = closest(device, ctx, wanted, 3);
    if close.is_empty() {
        return format!(
            "{} has no parameter {wanted:?}{within}, and the lock lists none for it; \
             knx_show_device says why",
            device.address
        );
    }
    let named: Vec<String> = close
        .iter()
        .map(|r| {
            let key = device
                .lock
                .parameters
                .get(*r)
                .map_or(*r, |p| p.key.as_str());
            format!("`{key}` on {}", place_of(device, ctx, r))
        })
        .collect();
    format!(
        "{} has no parameter {wanted:?}{within}. Closest: {}. {KEY_GRAMMAR}.",
        device.address,
        named.join(", ")
    )
}

/// The `n` lock refs whose key or vendor text is closest to `wanted`: a
/// prefix match first, then by edit distance.
fn closest<'a>(device: &'a Device, ctx: &KeyContext, wanted: &str, n: usize) -> Vec<&'a str> {
    let needle = slug(last_part(wanted));
    let mut scored: Vec<(bool, usize, &str, &str)> = device
        .lock
        .parameters
        .iter()
        .map(|(r, p)| {
            let key = last_part(&p.key);
            let text = ctx.texts.get(r).map(|t| slug(t)).unwrap_or_default();
            let prefix = key.starts_with(&needle)
                || (!text.is_empty() && text.starts_with(&needle))
                || needle.starts_with(key);
            let distance = edit_distance(&needle, key).min(if text.is_empty() {
                usize::MAX
            } else {
                edit_distance(&needle, &text)
            });
            (!prefix, distance, p.key.as_str(), r.as_str())
        })
        .collect();
    scored.sort();
    scored.into_iter().take(n).map(|(_, _, _, r)| r).collect()
}

/// The Levenshtein distance between `a` and `b`, by characters.
fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = i;
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (diagonal + usize::from(ca != *cb))
                .min(row[j] + 1)
                .min(above + 1);
            diagonal = above;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use bussard_model::ProductModel;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    const APP: &str = "M-00FA_A-0298-10-0000";

    const PRODUCT: &str = include_str!("../../bussard-model/tests/fixtures/blocks_app.yaml");

    /// The #298 weather station, channel A enabled, with a second parameter
    /// on A's page and a page-qualified key.
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
  { key = "a.dusk-threshold-in-lx", channel = "generic-0", ref = "P-12_R-12", param = "P-12" },
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

    fn setup() -> Result<(Model, KeyContext, IndividualAddress), Box<dyn std::error::Error>> {
        let files: BTreeMap<String, String> = [
            ("bussard.lock", LOCK),
            ("devices/1.1.30.toml", DEVICE),
            (
                "groups.toml",
                "groups = [\n  { address = \"4/1/5\", name = \"Brightness\", dpt = \"9.004\" },\n]\n",
            ),
            ("bussard.toml", "[connection]\ntransport = \"tunnel\"\n"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let model = Model::from_texts(&files)?;
        let mut products = ProductModels::default();
        products
            .by_app_ref
            .insert(APP.to_string(), ProductModel::from_yaml(PRODUCT, APP)?);
        let ia: IndividualAddress = "1.1.30".parse()?;
        let ctx = KeyContext::new(&model, &products, ia);
        Ok((model, ctx, ia))
    }

    fn device(model: &Model, ia: IndividualAddress) -> Result<&Device, String> {
        model
            .devices
            .get(&ia)
            .map(|d| &d.device)
            .ok_or_else(|| format!("no device {ia}"))
    }

    #[test]
    fn test_resolve_parameter_key_four_shapes() -> TestResult {
        let (model, ctx, ia) = setup()?;
        let d = device(&model, ia)?;
        let page = "universal-channel-a-function";
        for wanted in [
            "channel-function".to_string(),
            format!("{page}.channel-function"),
            "channel-function@P-11_R-11".to_string(),
            "P-11_R-11".to_string(),
            "generic-0.channel-function".to_string(),
        ] {
            let key = resolve_parameter_key(d, &ctx, &wanted, None)?;
            assert_eq!(ref_of(&key), Some("P-11_R-11"), "{wanted}");
        }
        // A page-qualified key, by itself, by its last part, and under its page.
        for wanted in [
            "a.dusk-threshold-in-lx".to_string(),
            "dusk-threshold-in-lx".to_string(),
            format!("{page}.dusk-threshold-in-lx"),
            format!("{page}.a.dusk-threshold-in-lx"),
        ] {
            let key = resolve_parameter_key(d, &ctx, &wanted, None)?;
            assert_eq!(ref_of(&key), Some("P-12_R-12"), "{wanted}");
        }
        Ok(())
    }

    #[test]
    fn test_resolve_parameter_key_page_handle_in_channel() -> TestResult {
        let (model, ctx, ia) = setup()?;
        let d = device(&model, ia)?;
        let key = resolve_parameter_key(
            d,
            &ctx,
            "channel-function",
            Some("universal-channel-a-function"),
        )?;
        assert_eq!(ref_of(&key), Some("P-11_R-11"));
        // The enable switch sits on the General page, not on channel A's.
        let wrong = resolve_parameter_key(
            d,
            &ctx,
            "universal-channel-b-enable",
            Some("universal-channel-a-function"),
        );
        assert!(
            wrong
                .as_ref()
                .is_err_and(|e| e.contains("in universal-channel-a-function")),
            "{wrong:?}"
        );
        let unknown = resolve_parameter_key(d, &ctx, "channel-function", Some("nowhere"));
        assert!(
            unknown.as_ref().is_err_and(|e| e.contains("general")
                && e.contains("generic-0")
                && e.contains("no channel or page \"nowhere\"")),
            "{unknown:?}"
        );
        Ok(())
    }

    #[test]
    fn test_resolve_parameter_key_suggests_the_closest_keys() -> TestResult {
        let (model, ctx, ia) = setup()?;
        let d = device(&model, ia)?;
        let err = resolve_parameter_key(d, &ctx, "chanel-funktion", None)
            .err()
            .ok_or("a misspelled key resolved")?;
        assert!(
            err.contains("Closest: `channel-function` on universal-channel-a-function"),
            "{err}"
        );
        assert_eq!(err.matches(" on ").count(), 3, "{err}");
        assert!(err.contains("bare reference"), "{err}");
        // A prefix of the vendor text ranks first.
        let err = resolve_parameter_key(d, &ctx, "dusk", None)
            .err()
            .ok_or("a prefix resolved")?;
        assert!(err.contains("Closest: `a.dusk-threshold-in-lx`"), "{err}");
        Ok(())
    }

    #[test]
    fn test_edit_distance_counts_edits() {
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("same", "same"), 0);
    }
}
