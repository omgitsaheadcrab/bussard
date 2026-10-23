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
use bussard_model::device_view::{DEVICE_SCOPE, parameter_pages, unindexed_parameters};
use bussard_model::param_model::ProductModels;
use bussard_model::schema::{Device, LockedParameter};
use bussard_model::{IndividualAddress, Model, param_mem_key, slug};

/// The key grammar, stated once for the tool descriptions, `how_to_change`
/// and the refusals.
pub const KEY_GRAMMAR: &str = "Parameter keys: the key knx_show_device lists \
     (`helligkeitsschwelle`, page-qualified `sollwerte.komfort`, or `key@reference`), that key \
     prefixed with its page or channel handle (`universalkanal-c4-funktion.helligkeitsschwelle`), \
     or the bare reference (`P-7305_R-7305`); `channel` takes a channel handle, `device` or a \
     page handle";

/// What the resolver knows about a device beyond its lock: the ETS page each
/// parameter ref sits on, the vendor text of each, and the parameters the
/// product model's pages hold that the lock does not index.
#[derive(Debug, Default)]
pub struct KeyContext {
    /// Page handle by app-relative `ParameterRef` id.
    pages: BTreeMap<String, String>,
    /// Vendor text by app-relative `ParameterRef` id.
    texts: HashMap<String, String>,
    /// The parameters the device's pages hold beyond the lock's index (on a
    /// page that was inactive at import, at their default), by ref.
    unindexed: BTreeMap<String, Unindexed>,
    /// The application program, when its product model is at hand.
    application: Option<String>,
}

/// A parameter the product model's pages hold and the lock does not index.
#[derive(Debug, Clone)]
struct Unindexed {
    /// The key `knx_show_device` prints for it.
    key: String,
    /// The channel id the importer would put it in: the device's only
    /// channel, if it has exactly one.
    channel: Option<String>,
}

impl KeyContext {
    /// The context of device `address` in `model`, from the product models
    /// in `products` (empty without one).
    pub fn new(model: &Model, products: &ProductModels, address: IndividualAddress) -> Self {
        let mut pages = parameter_pages(model, products, address).unwrap_or_default();
        let mut texts = HashMap::new();
        let mut unindexed = BTreeMap::new();
        let mut application = None;
        if let Some(loaded) = model.devices.get(&address) {
            let device = &loaded.device;
            let app = device
                .product
                .as_ref()
                .and_then(|p| p.application_ref.as_deref());
            let product = app.and_then(|a| products.get(a));
            if product.is_some() {
                application = app.map(str::to_string);
            }
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
            let only_channel = match device.channels.keys().collect::<Vec<_>>().as_slice() {
                [id] => Some((*id).clone()),
                _ => None,
            };
            for (reference, u) in unindexed_parameters(model, products, address).unwrap_or_default()
            {
                if let Some(page) = u.page {
                    pages.insert(reference.clone(), page);
                }
                if let Some(text) = u.text {
                    texts.insert(reference.clone(), text);
                }
                unindexed.insert(
                    reference,
                    Unindexed {
                        key: u.key,
                        channel: only_channel.clone(),
                    },
                );
            }
        }
        KeyContext {
            pages,
            texts,
            unindexed,
            application,
        }
    }
}

/// One parameter the resolver can name: a lock entry, or one the product
/// model's pages hold beyond it.
#[derive(Debug, Clone, Copy)]
struct Candidate<'a> {
    reference: &'a str,
    key: &'a str,
    channel: Option<&'a str>,
    /// Whether the lock indexes it.
    indexed: bool,
}

/// The candidate for `reference`: the lock's entry, else the product
/// model's.
fn candidate<'a>(
    device: &'a Device,
    ctx: &'a KeyContext,
    reference: &str,
) -> Option<Candidate<'a>> {
    if let Some((r, p)) = device.lock.parameters.get_key_value(reference) {
        return Some(Candidate {
            reference: r,
            key: &p.key,
            channel: p.channel.as_deref(),
            indexed: true,
        });
    }
    ctx.unindexed
        .get_key_value(reference)
        .map(|(r, u)| Candidate {
            reference: r,
            key: &u.key,
            channel: u.channel.as_deref(),
            indexed: false,
        })
}

/// Every parameter the resolver can name on `device`.
fn candidates<'a>(device: &'a Device, ctx: &'a KeyContext) -> Vec<Candidate<'a>> {
    let locked = device.lock.parameters.iter().map(|(r, p)| Candidate {
        reference: r,
        key: &p.key,
        channel: p.channel.as_deref(),
        indexed: true,
    });
    let more = ctx
        .unindexed
        .iter()
        .filter(|(r, _)| !device.lock.parameters.contains_key(*r))
        .map(|(r, u)| Candidate {
            reference: r,
            key: &u.key,
            channel: u.channel.as_deref(),
            indexed: false,
        });
    locked.chain(more).collect()
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

/// Whether `text` has the shape of an app-relative parameter ref:
/// `P-7305_R-7305`, `MD-1_M-3_MI-1_P-3_R-45` or `UP-12_R-3`.
fn looks_like_ref(text: &str) -> bool {
    let parts: Vec<&str> = text.split('_').collect();
    let well_formed = parts.iter().all(|part| {
        part.split_once('-').is_some_and(|(kind, n)| {
            !kind.is_empty()
                && kind.chars().all(|c| c.is_ascii_uppercase())
                && !n.is_empty()
                && n.chars().all(|c| c.is_ascii_digit())
        })
    });
    well_formed
        && parts
            .iter()
            .any(|p| p.starts_with("P-") || p.starts_with("UP-"))
}

/// The handle of the channel parameter ref `reference` sits in, or `device`.
fn channel_of(device: &Device, ctx: &KeyContext, reference: &str) -> String {
    candidate(device, ctx, reference)
        .and_then(|c| c.channel)
        .map(|id| device.channel_handle(id))
        .unwrap_or_else(|| DEVICE_SCOPE.to_string())
}

/// The page or channel handle `reference` is listed under: its page when
/// the device lists pages, else its channel (or `device`).
fn place_of(device: &Device, ctx: &KeyContext, reference: &str) -> String {
    ctx.pages
        .get(reference)
        .cloned()
        .unwrap_or_else(|| channel_of(device, ctx, reference))
}

/// Whether `scope` (a channel handle or id, `device`, or a page handle)
/// holds parameter ref `reference`.
fn in_scope(device: &Device, ctx: &KeyContext, reference: &str, scope: &str) -> bool {
    if ctx.pages.get(reference).is_some_and(|h| h == scope) {
        return true;
    }
    match candidate(device, ctx, reference).and_then(|c| c.channel) {
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

/// The refs the lock or the product model's pages list for `wanted` within
/// every scope of `scopes`. Both count the same: a key that names a
/// parameter the lock indexes and one it does not is ambiguous.
fn find<'a>(
    device: &'a Device,
    ctx: &'a KeyContext,
    wanted: &str,
    scopes: &[&str],
) -> Vec<&'a str> {
    let all = candidates(device, ctx);
    let fits = |r: &str| scopes.iter().all(|s| in_scope(device, ctx, r, s));
    // `key@reference` and the bare reference name one ref.
    let by_ref = wanted.rsplit_once('@').map_or(wanted, |(_, r)| r);
    if let Some(c) = candidate(device, ctx, by_ref) {
        return if fits(c.reference) {
            vec![c.reference]
        } else {
            Vec::new()
        };
    }
    let exact: Vec<&str> = all
        .iter()
        .filter(|c| c.key == wanted && fits(c.reference))
        .map(|c| c.reference)
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
    all.iter()
        .filter(|c| last_part(c.key) == tail && fits(c.reference))
        .map(|c| c.reference)
        .collect()
}

/// The in-memory key (`<slug>@<ref>`) of the parameter a caller names
/// `wanted` on `device`, in `channel` when given (see [`KEY_GRAMMAR`]).
/// A parameter the lock does not index (on a page that was inactive at
/// import) resolves from the product model like one it does.
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
    // A stored key whose ref is named (`key@reference`) and that neither the
    // lock nor the product model lists.
    let reference = ref_of(wanted).unwrap_or(wanted);
    if candidate(device, ctx, reference).is_none()
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
        [reference] => Ok(mem_key(device, ctx, reference)),
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

/// Indexes the parameter `mem_key` (as [`resolve_parameter_key`] returned
/// it) in `device`'s lock when the lock does not list it yet: a parameter on
/// a page that was inactive at import. The entry carries the key
/// `knx_show_device` printed (the escape hatch `<key>@<ref>` when another
/// entry of the channel ends in the same key), the device's only channel and
/// the parameter id, so the device file places the value like its
/// neighbours. Once the configuration shows the parameter, the facts refresh
/// keeps this entry, so the file does not move. Returns whether it added one.
pub fn index_parameter(device: &mut Device, ctx: &KeyContext, mem_key: &str) -> bool {
    let Some(reference) = ref_of(mem_key) else {
        return false;
    };
    if device.lock.parameters.contains_key(reference) {
        return false;
    }
    let Some(u) = ctx.unindexed.get(reference) else {
        return false;
    };
    let taken = device
        .lock
        .parameters
        .values()
        .any(|p| p.channel == u.channel && last_part(&p.key) == u.key)
        || device
            .com_objects
            .values()
            .any(|o| o.channel == u.channel && o.key.as_deref() == Some(u.key.as_str()));
    let key = if taken {
        format!("{}@{reference}", u.key)
    } else {
        u.key.clone()
    };
    device.lock.parameters.insert(
        reference.to_string(),
        LockedParameter {
            key,
            channel: u.channel.clone(),
            param: Some(ref_param_id(reference).to_string()),
            default: None,
        },
    );
    true
}

/// The key `reference` is stored under, else the one the loader derives.
fn mem_key(device: &Device, ctx: &KeyContext, reference: &str) -> String {
    device
        .parameters
        .keys()
        .find(|k| ref_of(k) == Some(reference))
        .cloned()
        .unwrap_or_else(|| {
            let key = candidate(device, ctx, reference).map_or(reference, |c| c.key);
            param_mem_key(key, reference)
        })
}

/// `<place>.<key>` for `reference`: a key that names it on its own.
fn qualified(device: &Device, ctx: &KeyContext, reference: &str) -> String {
    let key = candidate(device, ctx, reference).map_or(reference, |c| c.key);
    let place = place_of(device, ctx, reference);
    if key
        .strip_prefix(place.as_str())
        .is_some_and(|rest| rest.starts_with('.'))
    {
        return key.to_string();
    }
    format!("{place}.{key}")
}

/// The refusal for a key that matches nothing, with the three closest keys.
fn no_match(device: &Device, ctx: &KeyContext, wanted: &str, channel: Option<&str>) -> String {
    let within = channel.map(|c| format!(" in {c}")).unwrap_or_default();
    // A reference nothing knows: edit-distance suggestions would be noise.
    let reference = ref_of(wanted).unwrap_or(wanted);
    if looks_like_ref(reference) && candidate(device, ctx, reference).is_none() {
        return match &ctx.application {
            Some(app) => format!(
                "{} has no parameter with reference {reference} in its application {app}. \
                 {KEY_GRAMMAR}.",
                device.address
            ),
            None => format!(
                "{} has no parameter with reference {reference} in its lock, and no product \
                 model is at hand to look further. {KEY_GRAMMAR}.",
                device.address
            ),
        };
    }
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
        .map(|c| {
            let place = place_of(device, ctx, c.reference);
            if c.indexed {
                format!("`{}` on {place}", c.key)
            } else {
                format!(
                    "`{}` on {place} (at its default, not in the lock yet)",
                    c.key
                )
            }
        })
        .collect();
    format!(
        "{} has no parameter {wanted:?}{within}. Closest: {}. {KEY_GRAMMAR}.",
        device.address,
        named.join(", ")
    )
}

/// The `n` parameters whose key or vendor text is closest to `wanted`: a
/// prefix match first, then by edit distance.
fn closest<'a>(
    device: &'a Device,
    ctx: &'a KeyContext,
    wanted: &str,
    n: usize,
) -> Vec<Candidate<'a>> {
    let needle = slug(last_part(wanted));
    let mut scored: Vec<(bool, usize, &str, &str, Candidate<'a>)> = candidates(device, ctx)
        .into_iter()
        .map(|c| {
            let key = last_part(c.key);
            let text = ctx
                .texts
                .get(c.reference)
                .map(|t| slug(t))
                .unwrap_or_default();
            let prefix = key.starts_with(&needle)
                || (!text.is_empty() && text.starts_with(&needle))
                || needle.starts_with(key);
            let distance = edit_distance(&needle, key).min(if text.is_empty() {
                usize::MAX
            } else {
                edit_distance(&needle, &text)
            });
            (!prefix, distance, c.key, c.reference, c)
        })
        .collect();
    scored.sort_by(|a, b| (a.0, a.1, a.2, a.3).cmp(&(b.0, b.1, b.2, b.3)));
    scored.into_iter().take(n).map(|s| s.4).collect()
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
            format!("{page}.channel-function"),
            "channel-function@P-11_R-11".to_string(),
            "P-11_R-11".to_string(),
        ] {
            let key = resolve_parameter_key(d, &ctx, &wanted, None)?;
            assert_eq!(ref_of(&key), Some("P-11_R-11"), "{wanted}");
        }
        // A page-qualified key, by itself and under its page.
        for wanted in [
            "a.dusk-threshold-in-lx".to_string(),
            format!("{page}.dusk-threshold-in-lx"),
            format!("{page}.a.dusk-threshold-in-lx"),
        ] {
            let key = resolve_parameter_key(d, &ctx, &wanted, None)?;
            assert_eq!(ref_of(&key), Some("P-12_R-12"), "{wanted}");
        }
        Ok(())
    }

    #[test]
    fn test_resolve_parameter_key_bare_key_on_several_pages_is_ambiguous() -> TestResult {
        let (model, ctx, ia) = setup()?;
        let d = device(&model, ia)?;
        // Channels B and C have a "Channel function" too, on pages the lock
        // does not index: a bare key does not pick channel A silently.
        let err = resolve_parameter_key(d, &ctx, "channel-function", None)
            .err()
            .ok_or("a bare key on three pages resolved")?;
        for qualified in [
            "`universal-channel-a-function.channel-function`",
            "`universal-channel-b-function.channel-function`",
            "`universal-channel-c-function.channel-function`",
        ] {
            assert!(err.contains(qualified), "{err}");
        }
        // The channel handle alone does not tell them apart either.
        assert!(resolve_parameter_key(d, &ctx, "generic-0.channel-function", None).is_err());
        Ok(())
    }

    #[test]
    fn test_resolve_parameter_key_unindexed_in_every_shape() -> TestResult {
        let (model, ctx, ia) = setup()?;
        let d = device(&model, ia)?;
        let page = "universal-channel-b-function";
        for wanted in [
            format!("{page}.channel-function"),
            "channel-function@P-21_R-21".to_string(),
            "P-21_R-21".to_string(),
        ] {
            let key = resolve_parameter_key(d, &ctx, &wanted, None)?;
            assert_eq!(key, "channel-function@P-21_R-21", "{wanted}");
        }
        let key = resolve_parameter_key(d, &ctx, "channel-function", Some(page))?;
        assert_eq!(key, "channel-function@P-21_R-21");
        // The enable switch of channel C sits on the General page.
        let key = resolve_parameter_key(d, &ctx, "universal-channel-c-enable", None)?;
        assert_eq!(key, "universal-channel-c-enable@P-3_R-3");
        Ok(())
    }

    #[test]
    fn test_resolve_parameter_key_unknown_reference() -> TestResult {
        let (model, ctx, ia) = setup()?;
        let d = device(&model, ia)?;
        for wanted in ["P-7305_R-7305", "helligkeitsschwelle@P-7305_R-7305"] {
            let err = resolve_parameter_key(d, &ctx, wanted, None)
                .err()
                .ok_or("an unknown reference resolved")?;
            assert!(
                err.contains("no parameter with reference P-7305_R-7305 in its application"),
                "{err}"
            );
            assert!(!err.contains("Closest"), "{err}");
        }
        Ok(())
    }

    #[test]
    fn test_index_parameter_adds_the_lock_entry_once() -> TestResult {
        let (mut model, ctx, ia) = setup()?;
        let d = &mut model.devices.get_mut(&ia).ok_or("no device")?.device;
        let key = resolve_parameter_key(
            d,
            &ctx,
            "universal-channel-b-function.channel-function",
            None,
        )?;
        assert!(index_parameter(d, &ctx, &key));
        let entry = d.lock.parameters.get("P-21_R-21").ok_or("not indexed")?;
        // Channel A's "channel-function" holds the plain key in this channel.
        assert_eq!(entry.key, "channel-function@P-21_R-21");
        assert_eq!(entry.channel.as_deref(), Some("CH-0"));
        assert_eq!(entry.param.as_deref(), Some("P-21"));
        assert!(!index_parameter(d, &ctx, &key));
        let key = resolve_parameter_key(d, &ctx, "universal-channel-c-enable", None)?;
        assert!(index_parameter(d, &ctx, &key));
        let entry = d.lock.parameters.get("P-3_R-3").ok_or("not indexed")?;
        assert_eq!(entry.key, "universal-channel-c-enable");
        // An indexed parameter stays as the lock has it.
        assert!(!index_parameter(d, &ctx, "channel-function@P-11_R-11"));
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
