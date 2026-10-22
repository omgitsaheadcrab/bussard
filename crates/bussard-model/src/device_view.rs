//! What a device offers, in the device file's words: `bussard device` and the
//! MCP `knx_show_device` tool.
//!
//! [`device_view`] lists a device's channels (handle, vendor text, the user's
//! name) and, for one channel (or `device` for the device-level scope), its
//! parameters (key, current value, the enum choices or range from the product
//! model, whether it is at the vendor default) and objects (key, number,
//! text/function, DPT, flags, what it sends and listens on). The data comes
//! from the lock (joined into the in-memory [`Device`]) and the product models
//! under `models/`; without a product model the view says so and lists what
//! the lock has.
//!
//! For an application without channels of its own (none, or one generic
//! channel) the view also lists the ETS parameter pages of the product
//! model's `blocks` in ETS order, and for one with channels the channels its
//! configuration does not show (issue #298). A page or channel the
//! configuration does not show is marked inactive with the parameters that
//! enable it, the objects it would show and how the change reaches the
//! device; `summary` counts them. A page handle selects one page as the
//! scope.
//!
//! [`DeviceView::render_table`] is the terminal rendering, and
//! [`DeviceView::render_toml`] the paste-ready device-file snippet: what the
//! file sets, plus commented lines `# key = "value"   # choice | choice` for
//! what it could set.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Serialize;

use crate::address::IndividualAddress;
use crate::blocks::{
    BlockDef, BlockKind, BlockState, block_states, object_number, parent_indexes, ref_param_id,
    subtree,
};
use crate::flags::Flags;
use crate::loader::Model;
use crate::param_model::{ParamDef, ParamKind, ProductModel, ProductModels, enum_label};
use crate::schema::Device;

/// The channel argument that selects the device-level scope (`[parameters]`
/// and `[links]` of the device file).
pub const DEVICE_SCOPE: &str = "device";

/// One channel in the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChannelRow {
    /// The handle the device file uses (`[channel.<handle>]`).
    pub handle: String,
    /// The vendor's channel number, when given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<u32>,
    /// The vendor's channel text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The user's name for it, when it differs from the vendor text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// How many parameters the lock lists for it.
    pub parameters: usize,
    /// How many objects it has.
    pub objects: usize,
}

/// One parameter of a scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ParamRow {
    /// The key the device file uses.
    pub key: String,
    /// The app-relative `ParameterRef` id.
    pub reference: String,
    /// The vendor's text, from the product model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The value the device file sets, rendered as the file would write it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// The vendor default, rendered like `value`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// Whether the effective value is the vendor default; `None` when the
    /// product model is not at hand.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at_default: Option<bool>,
    /// The enum labels (or codes), for an enumeration.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
    /// The range of an integer, `min..max`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
}

/// One object of a scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ObjectRow {
    /// The key the device file uses, or the number when the lock gives none.
    pub key: String,
    /// The com-object number.
    pub number: u16,
    /// The vendor's text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The vendor's function text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    /// The datapoint type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dpt: Option<String>,
    /// The communication flags (`CWU`, `CRT`, …).
    pub flags: String,
    /// The group address it sends on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub send: Option<String>,
    /// The group addresses it listens on.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub listen: Vec<String>,
}

/// A parameter to set so the device shows an inactive block or channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnableStep {
    /// The key the device file uses, when the lock lists the parameter (one
    /// that the configuration does not show yet has none).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// The app-relative `ParameterRef` id.
    pub reference: String,
    /// The vendor's text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The value that shows the block.
    pub set_to: String,
    /// The vendor default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// The device's value now (stored, else the default).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
}

/// A com-object an inactive block would show once enabled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShownObject {
    /// The com-object number.
    pub number: u16,
    /// The vendor's text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The vendor's function text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    /// The datapoint type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dpt: Option<String>,
    /// The communication flags.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flags: Option<String>,
}

/// Why a block or channel is inactive and what enabling it takes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Inactive {
    /// The parameters to set, outermost first (a later one appears once the
    /// earlier ones are set).
    pub enable: Vec<EnableStep>,
    /// The com-objects it shows with the vendor defaults once enabled.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub would_show: Vec<ShownObject>,
    /// How the change reaches the device.
    pub needs: String,
}

/// One ETS parameter page (`<ParameterBlock>`) of a channel-less
/// application, or an inactive channel of an application with channels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlockRow {
    /// The handle `bussard device <ia> <handle>` takes: the slug of the
    /// title.
    pub handle: String,
    /// The app-relative element id (`PB-7243`, `CH-4`).
    pub id: String,
    /// The title ETS shows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// How deep it is nested in other pages (0: top level).
    pub depth: usize,
    /// Whether the configuration shows it.
    pub active: bool,
    /// Active: how many of the device's parameters sit directly on it.
    /// Inactive: how many it and its nested pages hold.
    pub parameters: usize,
    /// Active: how many of the device's objects sit directly on it.
    /// Inactive: how many it would show once enabled.
    pub objects: usize,
    /// Set for an inactive one: what enables it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inactive: Option<Inactive>,
    /// The ids of the pages folded into this row: inactive pages the same
    /// settings show (Theben's "Objekte zu C4" behind "Universalkanal C4").
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub includes: Vec<String>,
}

/// One scope in detail: a channel, or the device level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScopeView {
    /// The channel handle, `None` for the device level.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    /// The vendor's channel text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The user's channel name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The page handle, when the scope is an ETS parameter page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block: Option<String>,
    /// For an inactive page: what enables it. Its parameters are then the
    /// ones it would show (no key in the lock yet, no value), and its objects
    /// the ones it would show with the vendor defaults.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inactive: Option<Inactive>,
    /// Its parameters, by key.
    pub parameters: Vec<ParamRow>,
    /// Its objects, by number.
    pub objects: Vec<ObjectRow>,
}

/// A device as `bussard device` shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceView {
    /// The individual address.
    pub address: String,
    /// The device name.
    pub name: String,
    /// The order number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product: Option<String>,
    /// The application program the lock pins.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application: Option<String>,
    /// Whether the product model (`models/<application>.yaml`) was at hand.
    pub product_model: bool,
    /// Every channel.
    pub channels: Vec<ChannelRow>,
    /// How many device-level parameters and objects there are.
    pub device_level: (usize, usize),
    /// For an application without channels of its own (none, or one generic
    /// channel): its ETS parameter pages in ETS order, each active or
    /// inactive. Pages nested in an inactive page are not listed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<BlockRow>,
    /// For an application with channels: the channels its configuration does
    /// not show, with what enables each.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inactive_channels: Vec<BlockRow>,
    /// Spare capacity at a glance: how many pages or channels are inactive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// The scope asked for, in detail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<ScopeView>,
    /// What the view could not show, and why.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// Why a view could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ViewError {
    /// The model has no such device.
    #[error("{0} is not a device in this model")]
    NoDevice(IndividualAddress),
    /// The device has no such channel.
    #[error("{address} has no channel {channel:?}; its channels are: {known}")]
    NoChannel {
        /// The device.
        address: IndividualAddress,
        /// What was asked for.
        channel: String,
        /// The handles it has.
        known: String,
    },
}

/// One parameter the lock or the file knows, before rendering.
struct ParamEntry {
    reference: String,
    key: String,
    channel: Option<String>,
    param: Option<String>,
    stored: Option<String>,
    /// The ref's own default when it overrides the parameter's (the lock's
    /// `default`), else `None` and the product model's default applies.
    ref_default: Option<String>,
}

/// Every parameter of `device`: the lock's index plus what the file stores.
fn parameter_entries(device: &Device) -> Vec<ParamEntry> {
    let mut by_ref: BTreeMap<String, ParamEntry> = BTreeMap::new();
    for (reference, p) in &device.lock.parameters {
        by_ref.insert(
            reference.clone(),
            ParamEntry {
                reference: reference.clone(),
                key: p.key.clone(),
                channel: p.channel.clone(),
                param: p.param.clone(),
                stored: None,
                ref_default: p.default.clone(),
            },
        );
    }
    for (mem_key, value) in &device.parameters {
        let Some((_, reference)) = mem_key.split_once('@') else {
            continue;
        };
        let entry = by_ref
            .entry(reference.to_string())
            .or_insert_with(|| ParamEntry {
                reference: reference.to_string(),
                key: mem_key.clone(),
                channel: None,
                param: crate::param_model::key_to_param_id(mem_key),
                stored: None,
                ref_default: None,
            });
        entry.stored = Some(value.clone());
    }
    by_ref.into_values().collect()
}

/// Renders a stored or default code as the file writes it: the enum label
/// when there is a usable one, else the value as is.
fn display_value(def: Option<&ParamDef>, value: &str) -> String {
    match def {
        Some(d) if !d.labels.is_empty() => enum_label(&d.labels, value)
            .map(str::to_string)
            .unwrap_or_else(|| value.to_string()),
        _ => value.to_string(),
    }
}

/// Whether two renderings of a value mean the same (a label and its code).
fn same_value(def: Option<&ParamDef>, a: &str, b: &str) -> bool {
    let a = a.trim();
    let b = b.trim();
    if a == b {
        return true;
    }
    let Some(d) = def else { return false };
    let code = |v: &str| {
        v.parse::<i64>()
            .ok()
            .or_else(|| crate::param_model::enum_code(&d.labels, v))
    };
    matches!((code(a), code(b)), (Some(x), Some(y)) if x == y)
}

fn param_row(entry: &ParamEntry, product: Option<&ProductModel>) -> ParamRow {
    let def = entry
        .param
        .as_deref()
        .and_then(|id| product.and_then(|p| p.parameters.get(id)));
    let default = entry
        .ref_default
        .as_deref()
        .or_else(|| def.and_then(|d| d.default.as_deref()));
    let value = entry.stored.as_deref().map(|v| display_value(def, v));
    let at_default = def.map(|_| match (entry.stored.as_deref(), default) {
        (None, _) => true,
        (Some(v), Some(d)) => same_value(def, v, d),
        (Some(_), None) => false,
    });
    let choices = def
        .map(|d| {
            if d.labels.is_empty() {
                match &d.kind {
                    ParamKind::Enum { values } => values.iter().map(i64::to_string).collect(),
                    _ => Vec::new(),
                }
            } else {
                d.labels.iter().map(|(_, t)| t.clone()).collect()
            }
        })
        .unwrap_or_default();
    let range = def.and_then(|d| match &d.kind {
        ParamKind::Int { min, max, .. } => match (min, max) {
            (None, None) => None,
            (a, b) => Some(format!(
                "{}..{}",
                a.map(|v| v.to_string()).unwrap_or_default(),
                b.map(|v| v.to_string()).unwrap_or_default()
            )),
        },
        ParamKind::Text { len: Some(n) } => Some(format!("text, up to {n} bytes")),
        _ => None,
    });
    ParamRow {
        key: entry.key.clone(),
        reference: entry.reference.clone(),
        text: def.and_then(|d| d.text.clone()),
        value,
        default: default.map(|d| display_value(def, d)),
        at_default,
        choices,
        range,
    }
}

/// The numeric value of a stored or default parameter value: the number,
/// or the code of an enum label.
fn value_number(def: Option<&ParamDef>, value: &str) -> Option<f64> {
    let v = value.trim();
    v.parse::<f64>().ok().or_else(|| {
        def.and_then(|d| crate::param_model::enum_code(&d.labels, v))
            .map(|c| c as f64)
    })
}

/// The enum codes of a parameter, in order (empty for other kinds).
fn enum_codes(def: &ParamDef) -> Vec<i64> {
    match &def.kind {
        ParamKind::Enum { values } if !values.is_empty() => values.clone(),
        _ => def.labels.iter().map(|(c, _)| *c).collect(),
    }
}

/// The ETS parameter pages of one device: which it shows, and where its
/// parameters and objects sit.
struct Pages<'a> {
    product: &'a ProductModel,
    blocks: &'a [BlockDef],
    parents: Vec<Option<usize>>,
    states: Vec<BlockState>,
    /// The device's parameters by ref.
    entries: HashMap<&'a str, &'a ParamEntry>,
    /// The block each of the device's parameter refs sits on.
    param_block: HashMap<&'a str, usize>,
    /// The block each of the device's object refs sits on.
    object_block: HashMap<String, usize>,
}

impl<'a> Pages<'a> {
    fn new(device: &'a Device, product: &'a ProductModel, params: &'a [ParamEntry]) -> Self {
        let blocks = product.blocks.as_slice();
        // A stored value of a parameter the configuration hides (`hidden@…`)
        // is no evidence that its page is shown.
        let entries: HashMap<&str, &ParamEntry> = params
            .iter()
            .filter(|p| !crate::files::is_hidden_mem_key(&p.key))
            .map(|p| (p.reference.as_str(), p))
            .collect();
        let active_params: HashSet<&str> = entries.keys().copied().collect();
        let object_refs: Vec<&str> = device
            .com_objects
            .values()
            .filter_map(|o| o.reference.as_deref())
            .collect();
        let active_objects: HashSet<&str> = object_refs.iter().copied().collect();
        let value_of = |reference: &str| -> Option<f64> {
            let def = product.parameters.get(ref_param_id(reference));
            let raw = match entries.get(reference) {
                Some(e) => e
                    .stored
                    .clone()
                    .or_else(|| e.ref_default.clone())
                    .or_else(|| def.and_then(|d| d.default.clone())),
                None => def.and_then(|d| d.default.clone()),
            }?;
            value_number(def, &raw)
        };
        let states = block_states(blocks, &active_params, &active_objects, &value_of);
        let parents = parent_indexes(blocks);
        // Active blocks claim a ref first, so a parameter that alternative
        // pages share sits on the one the device shows.
        let order: Vec<usize> = (0..blocks.len())
            .filter(|i| states[*i].active)
            .chain((0..blocks.len()).filter(|i| !states[*i].active))
            .collect();
        let mut param_block: HashMap<&str, usize> = HashMap::new();
        let mut object_block: HashMap<String, usize> = HashMap::new();
        for &i in &order {
            for r in &blocks[i].parameters {
                if let Some((key, _)) = entries.get_key_value(r.as_str()) {
                    param_block.entry(*key).or_insert(i);
                }
            }
            for r in blocks[i].objects.iter().chain(&blocks[i].other_objects) {
                if active_objects.contains(r.as_str()) {
                    object_block.entry(r.clone()).or_insert(i);
                }
            }
        }
        Pages {
            product,
            blocks,
            parents,
            states,
            entries,
            param_block,
            object_block,
        }
    }

    /// How many `<ParameterBlock>` ancestors block `i` has.
    fn depth(&self, i: usize) -> usize {
        let mut depth = 0;
        let mut at = self.parents[i];
        while let Some(p) = at {
            if self.blocks[p].kind == BlockKind::Block {
                depth += 1;
            }
            at = self.parents[p];
        }
        depth
    }

    /// The device's value of a parameter ref, rendered as the file writes it.
    fn current(&self, reference: &str) -> Option<String> {
        let def = self.product.parameters.get(ref_param_id(reference));
        let raw = match self.entries.get(reference) {
            Some(e) => e
                .stored
                .clone()
                .or_else(|| e.ref_default.clone())
                .or_else(|| def.and_then(|d| d.default.clone())),
            None => def.and_then(|d| d.default.clone()),
        }?;
        Some(display_value(def, &raw))
    }

    /// The parameter definitions blocks `members` and their nested blocks
    /// hold, one per parameter, without headings (`type: none`).
    fn held_params(&self, members: &[usize]) -> Vec<(&'a str, &'a ParamDef)> {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut out = Vec::new();
        let all = members
            .iter()
            .flat_map(|i| subtree(self.blocks, &self.parents, *i));
        for j in all {
            for r in &self.blocks[j].parameters {
                let id = ref_param_id(r);
                let Some(def) = self.product.parameters.get(id) else {
                    continue;
                };
                if matches!(def.kind, ParamKind::None) || !seen.insert(id) {
                    continue;
                }
                out.push((r.as_str(), def));
            }
        }
        out
    }

    /// The objects blocks `members` show with the vendor defaults once
    /// enabled: their own, and those of nested blocks whose conditions hold.
    fn would_show(&self, members: &[usize]) -> Vec<ShownObject> {
        let mut shown: HashSet<usize> = HashSet::new();
        let mut refs: Vec<&str> = Vec::new();
        let all = members
            .iter()
            .flat_map(|i| subtree(self.blocks, &self.parents, *i));
        for j in all {
            let reached = members.contains(&j)
                || (self.parents[j].is_some_and(|p| shown.contains(&p))
                    && self.blocks[j]
                        .when
                        .iter()
                        .all(|g| g.holds(self.value_number(&g.reference))));
            if !reached {
                continue;
            }
            shown.insert(j);
            refs.extend(self.blocks[j].objects.iter().map(String::as_str));
        }
        let mut out: Vec<ShownObject> = Vec::new();
        for r in refs {
            let Some(number) = object_number(r) else {
                continue;
            };
            if out.iter().any(|o| o.number == number) {
                continue;
            }
            let o = self.product.object_def(r);
            out.push(ShownObject {
                number,
                text: o.and_then(|o| o.text.clone()),
                function: o.and_then(|o| o.function.clone()),
                dpt: o.and_then(|o| o.dpt.clone()),
                flags: o.and_then(|o| o.flags.clone()),
            });
        }
        out.sort_by_key(|o| o.number);
        out
    }

    fn value_number(&self, reference: &str) -> Option<f64> {
        let def = self.product.parameters.get(ref_param_id(reference));
        let raw = match self.entries.get(reference) {
            Some(e) => e
                .stored
                .clone()
                .or_else(|| e.ref_default.clone())
                .or_else(|| def.and_then(|d| d.default.clone())),
            None => def.and_then(|d| d.default.clone()),
        }?;
        value_number(def, &raw)
    }

    /// What enables inactive block `i` (shown together with `members`), for
    /// device `address`.
    fn inactive(&self, i: usize, members: &[usize], address: IndividualAddress) -> Inactive {
        let enable = self.states[i]
            .failing
            .iter()
            .filter_map(|n| self.blocks[i].when.get(*n))
            .map(|g| {
                let def = self.product.parameters.get(ref_param_id(&g.reference));
                let codes = def.map(enum_codes).unwrap_or_default();
                let set_to = g
                    .satisfying(&codes)
                    .first()
                    .copied()
                    .or_else(|| g.first_value())
                    .map(|c| display_value(def, &c.to_string()))
                    .unwrap_or_else(|| g.test.clone());
                EnableStep {
                    key: self
                        .entries
                        .get(g.reference.as_str())
                        .map(|e| e.key.clone()),
                    reference: g.reference.clone(),
                    text: def.and_then(|d| d.text.clone().or_else(|| d.name.clone())),
                    set_to,
                    default: def
                        .and_then(|d| d.default.as_deref())
                        .map(|d| display_value(def, d)),
                    current: self.current(&g.reference),
                }
            })
            .collect();
        let would_show = self.would_show(members);
        let needs = if would_show.is_empty() {
            format!(
                "set the parameter in the device file, then push it with knx_apply_device or \
                 `bussard apply {address}`"
            )
        } else {
            format!(
                "enabling it shows com-objects, so it needs `bussard flash {address}` (a \
                 parameter push does not change the object table)"
            )
        };
        Inactive {
            enable,
            would_show,
            needs,
        }
    }

    /// Whether the device has a parameter or object on active block `i` or a
    /// block nested in it. A page of only internal parameters
    /// (`Access="None"`, which the lock leaves out) shows nothing.
    fn shows_something(&self, i: usize) -> bool {
        let tree: HashSet<usize> = subtree(self.blocks, &self.parents, i).into_iter().collect();
        self.param_block.values().any(|b| tree.contains(b))
            || self.object_block.values().any(|b| tree.contains(b))
    }

    /// Whether block `i` (with its nested blocks) holds anything to show.
    fn has_content(&self, i: usize) -> bool {
        subtree(self.blocks, &self.parents, i).into_iter().any(|j| {
            let b = &self.blocks[j];
            !b.objects.is_empty()
                || !b.other_objects.is_empty()
                || b.parameters.iter().any(|r| {
                    self.product
                        .parameters
                        .get(ref_param_id(r))
                        .is_some_and(|d| !matches!(d.kind, ParamKind::None))
                })
        })
    }

    /// The row of block `i` (with the blocks folded into it, `members`)
    /// under `handle`.
    fn row(&self, listed: &Listed, address: IndividualAddress) -> BlockRow {
        let i = listed.index;
        let members = listed.members.as_slice();
        let b = &self.blocks[i];
        let active = self.states[i].active;
        let (parameters, objects, inactive) = if active {
            (
                self.param_block.values().filter(|j| **j == i).count(),
                self.object_block.values().filter(|j| **j == i).count(),
                None,
            )
        } else {
            let inactive = self.inactive(i, members, address);
            (
                self.held_params(members).len(),
                inactive.would_show.len(),
                Some(inactive),
            )
        };
        BlockRow {
            handle: listed.handle.clone(),
            id: b.id.clone(),
            text: b.text.clone(),
            depth: self.depth(i),
            active,
            parameters,
            objects,
            inactive,
            includes: members
                .iter()
                .filter(|m| **m != i)
                .map(|m| self.blocks[*m].id.clone())
                .collect(),
        }
    }

    /// The failing conditions of inactive block `i`, as (ref, test) pairs.
    fn failing(&self, i: usize) -> Vec<(&str, &str)> {
        self.states[i]
            .failing
            .iter()
            .filter_map(|n| self.blocks[i].when.get(*n))
            .map(|g| (g.reference.as_str(), g.test.as_str()))
            .collect()
    }

    /// The blocks a channel-less device lists, with their handles: every
    /// `<ParameterBlock>` with something to show, except those nested in an
    /// inactive one. An inactive block whose failing conditions extend those
    /// of an earlier inactive block with the same parent and a title of
    /// another shape (Theben's "Objekte" or "Sicherheit" pages behind a
    /// channel's switch) is folded into that block; when the conditions are
    /// the same, enabling the earlier block shows it too, so its parameters
    /// and objects count there.
    fn listed(&self, taken: &[String]) -> Vec<Listed> {
        let candidates: Vec<usize> = self
            .blocks
            .iter()
            .enumerate()
            .filter(|(i, b)| {
                b.kind == BlockKind::Block
                    && !self.states[*i].parent_inactive
                    && self.has_content(*i)
                    && (!self.states[*i].active || self.shows_something(*i))
            })
            .map(|(i, _)| i)
            .collect();
        let mut out: Vec<Listed> = Vec::new();
        for i in candidates {
            if !self.states[i].active {
                let mine = self.failing(i);
                let host = out.iter_mut().rev().find(|l| {
                    let a = l.index;
                    if self.states[a].active || self.parents[a] != self.parents[i] {
                        return false;
                    }
                    let theirs = self.failing(a);
                    mine.starts_with(&theirs)
                        && title_shape(self.blocks[a].text.as_deref())
                            != title_shape(self.blocks[i].text.as_deref())
                });
                if let Some(host) = host {
                    if self.failing(host.index) == mine {
                        host.members.push(i);
                    }
                    continue;
                }
            }
            out.push(Listed {
                index: i,
                handle: String::new(),
                members: vec![i],
            });
        }
        // Handles: the slug of the title, else of the name when titles
        // repeat, made unique with `-2`, `-3`, ….
        let slug_of = |t: Option<&str>| t.map(crate::slug).filter(|s| !s.is_empty());
        let mut title_count: HashMap<String, usize> = HashMap::new();
        for l in &out {
            if let Some(t) = slug_of(self.blocks[l.index].text.as_deref()) {
                *title_count.entry(t).or_default() += 1;
            }
        }
        let mut used: HashSet<String> = taken.iter().cloned().collect();
        used.insert(DEVICE_SCOPE.to_string());
        for l in &mut out {
            let b = &self.blocks[l.index];
            let title = slug_of(b.text.as_deref());
            let base = match (&title, slug_of(b.name.as_deref())) {
                (Some(t), Some(n)) if title_count.get(t).is_some_and(|c| *c > 1) => n,
                (Some(t), _) => t.clone(),
                (None, Some(n)) => n,
                (None, None) => b.id.to_lowercase(),
            };
            let mut handle = base.clone();
            let mut n = 2;
            while used.contains(&handle) {
                handle = format!("{base}-{n}");
                n += 1;
            }
            used.insert(handle.clone());
            l.handle = handle;
        }
        out
    }
}

/// A block a device lists: its index, handle and the blocks shown with it.
struct Listed {
    index: usize,
    handle: String,
    /// `index` and the inactive blocks folded into it that the same settings
    /// show.
    members: Vec<usize>,
}

/// A title with its numbering taken out: digit runs and single-letter words
/// become `#`, so "Universalkanal C4" and "Universalkanal C10", or "Channel
/// A" and "Channel B", have the same shape.
fn title_shape(text: Option<&str>) -> String {
    let text = text.unwrap_or("");
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let boundary_before = i == 0 || !chars[i - 1].is_alphanumeric();
        let boundary_after = chars.get(i + 1).is_none_or(|n| !n.is_alphanumeric());
        if c.is_ascii_digit() {
            while chars.get(i).is_some_and(char::is_ascii_digit) {
                i += 1;
            }
            out.push('#');
            continue;
        }
        if c.is_alphabetic() && boundary_before && boundary_after {
            out.push('#');
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

/// "17 of 40 pages inactive: 7 Universalkanal C4: Funktion .. Universalkanal
/// C10: Funktion; …": the inactive rows grouped by their title with the
/// numbers taken out.
fn summary(rows: &[BlockRow], what: &str) -> Option<String> {
    let top: Vec<&BlockRow> = rows.iter().filter(|r| !r.active).collect();
    if top.is_empty() {
        return None;
    }
    let mut groups: Vec<(String, Vec<&BlockRow>)> = Vec::new();
    for r in &top {
        let title = r.text.clone().unwrap_or_else(|| r.id.clone());
        let key = title_shape(Some(&title));
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, members)) => members.push(r),
            None => groups.push((key, vec![r])),
        }
    }
    let parts: Vec<String> = groups
        .iter()
        .map(|(_, members)| {
            let title = |r: &BlockRow| r.text.clone().unwrap_or_else(|| r.id.clone());
            match members.as_slice() {
                [one] => format!("1 {}", title(one)),
                [first, .., last] => {
                    format!("{} {} .. {}", members.len(), title(first), title(last))
                }
                [] => String::new(),
            }
        })
        .collect();
    Some(format!(
        "{} of {} {what} inactive: {}",
        top.len(),
        rows.len(),
        parts.join("; ")
    ))
}

/// One of the device's objects as a scope lists it, with its links.
fn object_row(
    number: u16,
    o: &crate::schema::ComObject,
    links: Option<&Vec<crate::schema::Link>>,
) -> ObjectRow {
    let link = links.and_then(|l| l.iter().find(|l| l.object == number));
    ObjectRow {
        key: o.key.clone().unwrap_or_else(|| number.to_string()),
        number,
        text: o.text.clone(),
        function: o.function.clone(),
        dpt: o.dpt.map(|d| d.to_string()),
        flags: o.flags.to_string(),
        send: link.and_then(|l| l.send).map(|g| g.to_string()),
        listen: link
            .map(|l| l.listen.iter().map(ToString::to_string).collect())
            .unwrap_or_default(),
    }
}

/// The scope of ETS parameter page `i`: the device's parameters and objects
/// on it and its nested pages, or, for an inactive page, the parameters and
/// objects it would show.
fn block_scope(
    pg: &Pages<'_>,
    device: &Device,
    links: Option<&Vec<crate::schema::Link>>,
    listed: &Listed,
    address: IndividualAddress,
) -> ScopeView {
    let i = listed.index;
    let handle = listed.handle.clone();
    let tree: HashSet<usize> = listed
        .members
        .iter()
        .flat_map(|m| subtree(pg.blocks, &pg.parents, *m))
        .collect();
    let text = pg.blocks[i].text.clone();
    if pg.states[i].active {
        let mut parameters: Vec<ParamRow> = pg
            .entries
            .iter()
            .filter(|(r, _)| pg.param_block.get(*r).is_some_and(|b| tree.contains(b)))
            .map(|(_, e)| param_row(e, Some(pg.product)))
            .collect();
        parameters.sort_by(|a, b| a.key.cmp(&b.key));
        let objects: Vec<ObjectRow> = device
            .com_objects
            .iter()
            .filter(|(_, o)| {
                o.reference
                    .as_ref()
                    .and_then(|r| pg.object_block.get(r))
                    .is_some_and(|b| tree.contains(b))
            })
            .map(|(n, o)| object_row(*n, o, links))
            .collect();
        return ScopeView {
            handle: None,
            text,
            name: None,
            block: Some(handle),
            inactive: None,
            parameters,
            objects,
        };
    }
    let parameters: Vec<ParamRow> = pg
        .held_params(&listed.members)
        .into_iter()
        .map(|(reference, def)| {
            let entry = ParamEntry {
                reference: reference.to_string(),
                key: crate::slug(
                    def.text
                        .as_deref()
                        .or(def.name.as_deref())
                        .unwrap_or(reference),
                ),
                channel: None,
                param: Some(ref_param_id(reference).to_string()),
                stored: None,
                ref_default: None,
            };
            param_row(&entry, Some(pg.product))
        })
        .collect();
    let inactive = pg.inactive(i, &listed.members, address);
    let objects: Vec<ObjectRow> = inactive
        .would_show
        .iter()
        .map(|o| ObjectRow {
            key: o.number.to_string(),
            number: o.number,
            text: o.text.clone(),
            function: o.function.clone(),
            dpt: o.dpt.clone(),
            flags: o.flags.clone().unwrap_or_default(),
            send: None,
            listen: Vec::new(),
        })
        .collect();
    ScopeView {
        handle: None,
        text,
        name: None,
        block: Some(handle),
        inactive: Some(inactive),
        parameters,
        objects,
    }
}

/// Builds the view of `address`, with `channel` (a handle, a channel id, the
/// vendor number, or [`DEVICE_SCOPE`]) in detail when given.
pub fn device_view(
    model: &Model,
    products: &ProductModels,
    address: IndividualAddress,
    channel: Option<&str>,
) -> Result<DeviceView, ViewError> {
    let device = &model
        .devices
        .get(&address)
        .ok_or(ViewError::NoDevice(address))?
        .device;
    let application = device
        .product
        .as_ref()
        .and_then(|p| p.application_ref.clone());
    let product = application.as_deref().and_then(|a| products.get(a));
    let params = parameter_entries(device);
    let links = model.links.links.get(&address);

    let user_name = |id: &str| {
        let ch = device.channels.get(id)?;
        let name = ch.name.trim();
        (!name.is_empty() && Some(name) != ch.text.as_deref().map(str::trim))
            .then(|| name.to_string())
    };
    let channels: Vec<ChannelRow> = device
        .channels
        .iter()
        .map(|(id, ch)| ChannelRow {
            handle: device.channel_handle(id),
            number: ch.number,
            text: ch.text.clone(),
            name: user_name(id),
            parameters: params
                .iter()
                .filter(|p| p.channel.as_deref() == Some(id))
                .count(),
            objects: device
                .com_objects
                .values()
                .filter(|o| o.channel.as_deref() == Some(id))
                .count(),
        })
        .collect();
    let device_level = (
        params.iter().filter(|p| p.channel.is_none()).count(),
        device
            .com_objects
            .values()
            .filter(|o| o.channel.is_none())
            .count(),
    );

    // The ETS parameter pages: listed for a device without channels of its
    // own, and the inactive channels of one with channels (issue #298).
    let pages = product
        .filter(|p| !p.blocks.is_empty())
        .map(|p| Pages::new(device, p, &params));
    let channel_less = device.channels.len() <= 1;
    let listed: Vec<Listed> = match &pages {
        Some(pg) if channel_less => pg.listed(
            &channels
                .iter()
                .map(|c| c.handle.clone())
                .collect::<Vec<_>>(),
        ),
        _ => Vec::new(),
    };
    let blocks: Vec<BlockRow> = match &pages {
        Some(pg) => listed.iter().map(|l| pg.row(l, address)).collect(),
        None => Vec::new(),
    };
    let inactive_channels: Vec<BlockRow> = match &pages {
        Some(pg) if !channel_less => pg
            .blocks
            .iter()
            .enumerate()
            .filter(|(i, b)| {
                b.kind == BlockKind::Channel
                    && !pg.states[*i].active
                    && !pg.states[*i].parent_inactive
                    && !device.channels.contains_key(&b.id)
                    && pg.has_content(*i)
            })
            .map(|(i, b)| {
                let handle = b
                    .text
                    .as_deref()
                    .map(crate::slug)
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| b.id.to_lowercase());
                pg.row(
                    &Listed {
                        index: i,
                        handle,
                        members: vec![i],
                    },
                    address,
                )
            })
            .collect(),
        _ => Vec::new(),
    };
    let summary = summary(&blocks, "pages").or_else(|| {
        (!inactive_channels.is_empty()).then(|| {
            let names: Vec<String> = inactive_channels
                .iter()
                .map(|r| r.text.clone().unwrap_or_else(|| r.id.clone()))
                .collect();
            format!(
                "{} channel(s) inactive: {}",
                inactive_channels.len(),
                names.join(", ")
            )
        })
    });
    let block_hit = channel.and_then(|wanted| {
        let pg = pages.as_ref()?;
        let hit = listed
            .iter()
            .find(|l| l.handle == wanted || pg.blocks[l.index].id == wanted)?;
        Some((pg, hit))
    });

    let scope = match channel {
        None => None,
        Some(_) if block_hit.is_some() => {
            block_hit.map(|(pg, hit)| block_scope(pg, device, links, hit, address))
        }
        Some(wanted) => {
            let id: Option<String> = if wanted == DEVICE_SCOPE {
                None
            } else {
                let found = device.channels.iter().find(|(id, ch)| {
                    device.channel_handle(id) == wanted
                        || id.as_str() == wanted
                        || ch.number.is_some_and(|n| n.to_string() == wanted)
                });
                match found {
                    Some((id, _)) => Some(id.clone()),
                    None => {
                        let mut known: Vec<String> =
                            channels.iter().map(|c| c.handle.clone()).collect();
                        known.push(DEVICE_SCOPE.to_string());
                        known.extend(listed.iter().map(|l| l.handle.clone()));
                        return Err(ViewError::NoChannel {
                            address,
                            channel: wanted.to_string(),
                            known: known.join(", "),
                        });
                    }
                }
            };
            let parameters: Vec<ParamRow> = params
                .iter()
                .filter(|p| p.channel == id)
                .map(|p| param_row(p, product))
                .collect();
            let objects: Vec<ObjectRow> = device
                .com_objects
                .iter()
                .filter(|(_, o)| o.channel == id)
                .map(|(number, o)| object_row(*number, o, links))
                .collect();
            Some(ScopeView {
                handle: id.as_deref().map(|i| device.channel_handle(i)),
                text: id
                    .as_deref()
                    .and_then(|i| device.channels.get(i))
                    .and_then(|c| c.text.clone()),
                name: id.as_deref().and_then(user_name),
                block: None,
                inactive: None,
                parameters,
                objects,
            })
        }
    };

    let mut notes = Vec::new();
    if product.is_none() {
        notes.push(match &application {
            Some(app) => match crate::lock_products::archive_file_for(
                device.lock.product_entry.as_ref(),
                [],
                app,
            ) {
                Some(archive) => format!(
                    "no product data: .bussard/models/{app}.yaml is missing, so there are no \
                     choices, ranges or defaults. It regenerates automatically from {archive}; \
                     if this note stays, that archive could not be read (bussard warns why). \
                     Showing what the lock has"
                ),
                None => format!(
                    "no product data: no stored archive carries {app}, so there are no choices, \
                     ranges or defaults (run `bussard import-product --order-number <order>` or \
                     re-run `bussard import`); showing what the lock has"
                ),
            },
            None => "no product data: the lock pins no application for this device; showing \
                     what the lock has"
                .to_string(),
        });
    }
    Ok(DeviceView {
        address: address.to_string(),
        name: device.name.clone(),
        product: device.product.as_ref().and_then(|p| p.order_number.clone()),
        application,
        product_model: product.is_some(),
        channels,
        device_level,
        blocks,
        inactive_channels,
        summary,
        scope,
        notes,
    })
}

/// Pads `rows` into columns under `header`.
fn table(header: &[&str], rows: &[Vec<String>]) -> String {
    let rows: Vec<Vec<String>> = rows
        .iter()
        .map(|r| r.iter().map(|c| one_line(c)).collect())
        .collect();
    let rows = rows.as_slice();
    let mut widths: Vec<usize> = header.iter().map(|h| h.chars().count()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if let Some(w) = widths.get_mut(i) {
                *w = (*w).max(cell.chars().count());
            }
        }
    }
    let line = |cells: Vec<String>| {
        let mut out = String::from("  ");
        for (i, cell) in cells.iter().enumerate() {
            let last = i + 1 == cells.len();
            if last {
                out.push_str(cell);
            } else {
                let pad = widths[i].saturating_sub(cell.chars().count());
                out.push_str(cell);
                out.push_str(&" ".repeat(pad + 2));
            }
        }
        format!("{}\n", out.trim_end())
    };
    let mut out = line(header.iter().map(|h| h.to_string()).collect());
    for row in rows {
        out.push_str(&line(row.clone()));
    }
    out
}

/// A vendor text on one line: a line break becomes a space (a parameter
/// text like "Soll ein zweites Telegramm\ngesendet werden ?" would break the
/// table).
fn one_line(text: &str) -> String {
    text.replace("\r\n", " ").replace(['\n', '\r'], " ")
}

/// A TOML key as the device file writes it: bare when TOML allows, quoted
/// otherwise; a page-qualified key (`sollwerte.komfort`) stays dotted.
fn toml_key(key: &str) -> String {
    let bare = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    };
    if bare(key) {
        return key.to_string();
    }
    if !key.contains('@') && key.split('.').all(bare) {
        return key.to_string();
    }
    toml_string(key)
}

/// A TOML basic string.
fn toml_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// What enables an inactive page or channel, in one line.
fn inactive_line(inactive: &Inactive) -> String {
    let steps: Vec<String> = inactive
        .enable
        .iter()
        .map(|e| {
            let name = e
                .key
                .clone()
                .or_else(|| e.text.clone().map(|t| format!("{t:?}")))
                .unwrap_or_else(|| e.reference.clone());
            match &e.default {
                Some(d) => format!("{name} = {:?} (default {d:?})", e.set_to),
                None => format!("{name} = {:?}", e.set_to),
            }
        })
        .collect();
    let mut line = if steps.is_empty() {
        "not shown under the current configuration".to_string()
    } else {
        format!("set {}", steps.join(", then "))
    };
    if !inactive.would_show.is_empty() {
        line.push_str(&format!("; shows {} object(s)", inactive.would_show.len()));
    }
    line.push_str(&format!("; {}", inactive.needs));
    line
}

/// The page or channel rows as a table.
fn block_table(rows: &[BlockRow]) -> String {
    let rows: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            vec![
                format!("{}{}", "  ".repeat(r.depth), r.handle),
                r.text.clone().unwrap_or_default(),
                r.parameters.to_string(),
                r.objects.to_string(),
                match &r.inactive {
                    Some(i) => format!("inactive: {}", inactive_line(i)),
                    None => "active".to_string(),
                },
            ]
        })
        .collect();
    table(&["page", "text", "params", "objects", "state"], &rows)
}

impl DeviceView {
    /// The terminal rendering: the channel table, then the scope in detail.
    pub fn render_table(&self) -> String {
        let mut out = format!("{} {}", self.address, self.name.trim());
        if let Some(p) = &self.product {
            out.push_str(&format!("  ({p}"));
            if let Some(app) = &self.application {
                out.push_str(&format!(", {app}"));
            }
            out.push(')');
        }
        out.push('\n');
        for note in &self.notes {
            out.push_str(&format!("note: {note}\n"));
        }
        match &self.scope {
            None => {
                if self.channels.is_empty() {
                    out.push_str("no channels\n");
                } else {
                    let rows: Vec<Vec<String>> = self
                        .channels
                        .iter()
                        .map(|c| {
                            vec![
                                c.handle.clone(),
                                c.number.map(|n| n.to_string()).unwrap_or_default(),
                                c.text.clone().unwrap_or_default(),
                                c.name.clone().unwrap_or_default(),
                                c.parameters.to_string(),
                                c.objects.to_string(),
                            ]
                        })
                        .collect();
                    out.push_str(&table(
                        &["channel", "no.", "vendor text", "name", "params", "objects"],
                        &rows,
                    ));
                }
                out.push_str(&format!(
                    "device level: {} parameter(s), {} object(s)\n",
                    self.device_level.0, self.device_level.1
                ));
                if !self.blocks.is_empty() {
                    out.push_str("pages (ETS parameter blocks, in ETS order):\n");
                    out.push_str(&block_table(&self.blocks));
                }
                if !self.inactive_channels.is_empty() {
                    out.push_str("inactive channels:\n");
                    out.push_str(&block_table(&self.inactive_channels));
                }
                if let Some(summary) = &self.summary {
                    out.push_str(&format!("summary: {summary}\n"));
                }
                out.push_str(&format!(
                    "`bussard device {} <channel>` lists a channel's parameters and objects \
                     (`{DEVICE_SCOPE}` for the device level{}); `--toml` prints them ready to \
                     paste.\n",
                    self.address,
                    if self.blocks.is_empty() {
                        ""
                    } else {
                        ", a page handle for one page"
                    }
                ));
            }
            Some(scope) => {
                match &scope.handle {
                    Some(h) => {
                        out.push_str(&format!("channel {h}"));
                        if let Some(t) = &scope.text {
                            out.push_str(&format!(" ({t})"));
                        }
                        if let Some(n) = &scope.name {
                            out.push_str(&format!(", named {n:?}"));
                        }
                        out.push('\n');
                    }
                    None => match &scope.block {
                        Some(b) => {
                            out.push_str(&format!("page {b}"));
                            if let Some(t) = &scope.text {
                                out.push_str(&format!(" ({t})"));
                            }
                            out.push('\n');
                        }
                        None => out.push_str("device level\n"),
                    },
                }
                if let Some(inactive) = &scope.inactive {
                    out.push_str(&format!("inactive: {}\n", inactive_line(inactive)));
                    out.push_str(
                        "it would show these parameters (at their defaults) and objects:\n",
                    );
                }
                out.push_str("parameters:\n");
                if scope.parameters.is_empty() {
                    out.push_str("  (none)\n");
                } else {
                    let rows: Vec<Vec<String>> = scope
                        .parameters
                        .iter()
                        .map(|p| {
                            let value = p
                                .value
                                .clone()
                                .or_else(|| p.default.clone())
                                .unwrap_or_else(|| "-".to_string());
                            let marker = match p.at_default {
                                Some(true) => "default",
                                Some(false) => "set",
                                None if p.value.is_some() => "set",
                                None => "",
                            };
                            let allowed = if !p.choices.is_empty() {
                                p.choices.join(" | ")
                            } else {
                                p.range.clone().unwrap_or_default()
                            };
                            vec![
                                p.key.clone(),
                                value,
                                marker.to_string(),
                                allowed,
                                p.text.clone().unwrap_or_default(),
                            ]
                        })
                        .collect();
                    out.push_str(&table(
                        &["key", "value", "", "choices / range", "text"],
                        &rows,
                    ));
                }
                out.push_str("objects:\n");
                if scope.objects.is_empty() {
                    out.push_str("  (none)\n");
                } else {
                    let rows: Vec<Vec<String>> = scope
                        .objects
                        .iter()
                        .map(|o| {
                            let mut links = Vec::new();
                            if let Some(s) = &o.send {
                                links.push(format!("sends {s}"));
                            }
                            if !o.listen.is_empty() {
                                links.push(format!("listens on {}", o.listen.join(", ")));
                            }
                            let label = match (&o.text, &o.function) {
                                (Some(t), Some(f)) => format!("{t}: {f}"),
                                (Some(t), None) => t.clone(),
                                (None, Some(f)) => f.clone(),
                                (None, None) => String::new(),
                            };
                            vec![
                                o.key.clone(),
                                o.number.to_string(),
                                label,
                                o.dpt.clone().unwrap_or_default(),
                                o.flags.clone(),
                                links.join("; "),
                            ]
                        })
                        .collect();
                    out.push_str(&table(
                        &["key", "no.", "text", "dpt", "flags", "links"],
                        &rows,
                    ));
                }
            }
        }
        out
    }

    /// The paste-ready device-file snippet for the scope (or, without one,
    /// the channel tables with their names).
    pub fn render_toml(&self) -> String {
        let mut out = format!(
            "# devices/{}.toml: {}; uncomment what you want to set\n",
            self.address,
            self.name.trim()
        );
        for note in &self.notes {
            out.push_str(&format!("# note: {note}\n"));
        }
        let Some(scope) = &self.scope else {
            for c in &self.channels {
                out.push_str(&format!("\n[channel.{}]", toml_key(&c.handle)));
                if let Some(t) = &c.text {
                    out.push_str(&format!("   # {t}"));
                }
                out.push('\n');
                match &c.name {
                    Some(n) => out.push_str(&format!("name = {}\n", toml_string(n))),
                    None => out.push_str(&format!(
                        "# name = {}\n",
                        toml_string(c.text.as_deref().unwrap_or(""))
                    )),
                }
            }
            return out;
        };
        match &scope.handle {
            Some(h) => {
                out.push_str(&format!("\n[channel.{}]", toml_key(h)));
                if let Some(t) = &scope.text {
                    out.push_str(&format!("   # {t}"));
                }
                out.push('\n');
                match &scope.name {
                    Some(n) => out.push_str(&format!("name = {}\n", toml_string(n))),
                    None => out.push_str(&format!(
                        "# name = {}\n",
                        toml_string(scope.text.as_deref().unwrap_or(""))
                    )),
                }
            }
            None => out.push_str("\n[parameters]\n"),
        }
        for p in &scope.parameters {
            let hint = if !p.choices.is_empty() {
                p.choices.join(" | ")
            } else {
                p.range.clone().unwrap_or_default()
            };
            let hint = match (&p.text, hint.is_empty()) {
                (Some(t), false) => format!("{t}: {hint}"),
                (Some(t), true) => t.clone(),
                (None, _) => hint,
            };
            let comment = if hint.is_empty() {
                String::new()
            } else {
                format!("   # {hint}")
            };
            match &p.value {
                Some(v) => out.push_str(&format!(
                    "{} = {}{comment}\n",
                    toml_key(&p.key),
                    toml_string(v)
                )),
                None => out.push_str(&format!(
                    "# {} = {}{comment}\n",
                    toml_key(&p.key),
                    toml_string(p.default.as_deref().unwrap_or(""))
                )),
            }
        }
        if scope.handle.is_none() && !scope.objects.is_empty() {
            out.push_str("\n[links]\n");
        }
        for o in &scope.objects {
            let flags: Flags = o.flags.parse().unwrap_or_default();
            let what = [o.function.as_deref(), o.text.as_deref(), o.dpt.as_deref()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(", ");
            let comment = format!("   # {} {what}", o.flags);
            let key = toml_key(&o.key);
            if o.send.is_none() && o.listen.is_empty() {
                // Commands take `listen` (W flag), status objects `send` (T).
                if flags.contains(Flags::WRITE) {
                    out.push_str(&format!("# {key}.listen = []{comment}\n"));
                } else if flags.contains(Flags::TRANSMIT) {
                    out.push_str(&format!("# {key}.send = \"\"{comment}\n"));
                }
                continue;
            }
            if let Some(s) = &o.send {
                out.push_str(&format!("{key}.send = {}{comment}\n", toml_string(s)));
            }
            if !o.listen.is_empty() {
                let list: Vec<String> = o.listen.iter().map(|g| toml_string(g)).collect();
                out.push_str(&format!("{key}.listen = [{}]{comment}\n", list.join(", ")));
            }
        }
        out
    }
}
