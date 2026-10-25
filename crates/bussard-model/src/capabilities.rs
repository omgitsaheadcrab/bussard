//! What the installed devices could do (issue #298): a search over the
//! parameter texts and enum choice texts of their product models, with
//! whether the page holding each match is active on the device.
//!
//! "Is there a dark bit?" is rarely a com-object the installation already
//! has: on a Theben Meteodata it is "Funktion des Kanals = Helligkeitssensor"
//! on a universal channel the project never enabled. [`capability_lookup`]
//! finds such a choice on every device whose product model offers it and
//! says, per match, whether the page or channel holding it is active and,
//! when it is not, what enables it (the [`crate::device_view`] analysis).
//!
//! Parsing a product model's YAML is slow next to a search; the models come
//! through [`ProductModels::load_apps`], which memoizes them and caches them
//! as JSON under [`crate::param_model::MODEL_CACHE_DIR`] (issue #339).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Serialize;

use crate::address::IndividualAddress;
use crate::blocks::ref_param_id;
use crate::device_view::{BlockRow, EnableStep, device_view};
use crate::loader::Model;
use crate::param_model::{ParamDef, ParamKind, ProductModel, ProductModels};

/// One matching parameter, or one of its choices, on a page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Match {
    /// The parameter's text.
    pub parameter: String,
    /// The key the device file uses, when the configuration shows the
    /// parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// The matching enum choice, when the match is a choice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choice: Option<String>,
    /// Whether the device has it now: the parameter is shown and, for a
    /// choice, set to it.
    pub selected: bool,
}

/// What one page or channel of a device could do: the parameters and
/// choices on it whose texts match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Capability {
    /// The device.
    pub device_ia: String,
    /// The device name.
    pub device_name: String,
    /// The ETS page or channel holding the parameter, by its title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_or_channel: Option<String>,
    /// Its handle for `knx_show_device` / `bussard device <ia> <handle>`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    /// The best match's parameter text (a selected one first, then a
    /// choice).
    pub parameter: String,
    /// The best match's key, when the configuration shows the parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// The best match's choice, when it is a choice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choice: Option<String>,
    /// Whether the page or channel is active on the device.
    pub active: bool,
    /// Whether the device has the best match now.
    pub selected: bool,
    /// `provides` (active, and the device has a match now) or `could
    /// provide` (on an inactive page, or another choice is set).
    pub status: String,
    /// The other matches on the same page (at most 5).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub more: Vec<Match>,
    /// For an inactive page: the parameters that enable it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub enable: Vec<EnableStep>,
    /// For an inactive page: how the change reaches the device.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub needs: Option<String>,
}

/// The other matches a [`Capability`] lists.
const MORE: usize = 5;

/// Lowercased with the German umlauts spelled out, so "Dämmerung" and
/// "daemmerung" find each other.
fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        match c {
            'ä' => out.push_str("ae"),
            'ö' => out.push_str("oe"),
            'ü' => out.push_str("ue"),
            'ß' => out.push_str("ss"),
            c => out.push(c),
        }
    }
    out
}

/// The text a parameter shows as.
fn param_text(def: &ParamDef) -> Option<&str> {
    def.text
        .as_deref()
        .or(def.name.as_deref())
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

/// A parameter of a product model whose texts match the query.
struct Hit<'a> {
    /// The block holding it, `None` for one no app-level page holds (a
    /// module parameter).
    block: Option<&'a str>,
    reference: &'a str,
    def: &'a ParamDef,
    text: &'a str,
    /// The matching choices, `(code, label)`.
    choices: Vec<(i64, &'a str)>,
    /// The parameter text itself matches (with no choice matching).
    plain: bool,
}

/// The parameters of `product` whose texts match `words`: every word in the
/// titles of the page path, the parameter text and the choice text
/// together, at least one in the parameter or choice text.
fn product_hits<'a>(product: &'a ProductModel, words: &[String]) -> Vec<Hit<'a>> {
    let titles: HashMap<&str, (Option<&str>, String)> = product
        .blocks()
        .iter()
        .map(|b| {
            (
                b.id.as_str(),
                (b.parent.as_deref(), fold(b.text.as_deref().unwrap_or(""))),
            )
        })
        .collect();
    let path_title = |block: Option<&str>| -> String {
        let mut out = String::new();
        let mut at = block;
        while let Some(id) = at {
            let Some((parent, title)) = titles.get(id) else {
                break;
            };
            out.push_str(title);
            out.push(' ');
            at = *parent;
        }
        out
    };
    let mut candidates: Vec<(Option<&str>, &str, &ParamDef)> = Vec::new();
    let mut held: HashSet<&str> = HashSet::new();
    for b in product.blocks() {
        let mut own: HashSet<&str> = HashSet::new();
        for r in &b.parameters {
            let id = ref_param_id(r);
            let Some(def) = product.parameters.get(id) else {
                continue;
            };
            held.insert(id);
            if own.insert(id) {
                candidates.push((Some(b.id.as_str()), r.as_str(), def));
            }
        }
    }
    for (id, def) in &product.parameters {
        if !held.contains(id.as_str()) {
            candidates.push((None, id.as_str(), def));
        }
    }
    let mut out = Vec::new();
    let mut page_cache: HashMap<Option<&str>, String> = HashMap::new();
    for (block, reference, def) in candidates {
        if matches!(def.kind, ParamKind::None) {
            continue;
        }
        let Some(text) = param_text(def) else {
            continue;
        };
        let pfold = fold(text);
        let page = page_cache
            .entry(block)
            .or_insert_with(|| path_title(block))
            .clone();
        let covered = |extra: &str| {
            words.iter().all(|w| {
                page.contains(w.as_str())
                    || pfold.contains(w.as_str())
                    || extra.contains(w.as_str())
            })
        };
        let choices: Vec<(i64, &str)> = def
            .labels
            .iter()
            .filter(|(_, label)| {
                let cfold = fold(label);
                covered(&cfold) && words.iter().any(|w| cfold.contains(w.as_str()))
            })
            .map(|(code, label)| (*code, label.as_str()))
            .collect();
        let plain =
            choices.is_empty() && covered("") && words.iter().any(|w| pfold.contains(w.as_str()));
        if plain || !choices.is_empty() {
            out.push(Hit {
                block,
                reference,
                def,
                text,
                choices,
                plain,
            });
        }
    }
    out
}

/// Searches the parameter texts and enum choice texts of every device's
/// product model for `query` (every word must appear in the page titles,
/// parameter text and choice text together, at least one in the parameter
/// or choice text; case and umlaut spelling do not matter). Returns at most
/// `limit` pages or channels, each with its matches: those where a device
/// provides a match now first, then those on an inactive page (spare
/// capacity), then active pages set to something else.
pub fn capability_lookup(model: &Model, dir: &Path, query: &str, limit: usize) -> Vec<Capability> {
    let words: Vec<String> = fold(query).split_whitespace().map(str::to_string).collect();
    if words.is_empty() || limit == 0 {
        return Vec::new();
    }
    let apps: HashSet<&str> = model
        .devices
        .values()
        .filter_map(|d| d.device.product.as_ref()?.application_ref.as_deref())
        .collect();
    let products = ProductModels::load_apps(dir, apps);
    let hits: HashMap<&str, Vec<Hit<'_>>> = products
        .by_app_ref
        .iter()
        .map(|(app, m)| (app.as_str(), product_hits(m, &words)))
        .filter(|(_, h)| !h.is_empty())
        .collect();

    let mut out: Vec<(u8, Capability)> = Vec::new();
    for (ia, loaded) in &model.devices {
        let Some(app) = loaded
            .device
            .product
            .as_ref()
            .and_then(|p| p.application_ref.as_deref())
        else {
            continue;
        };
        let (Some(product), Some(app_hits)) = (products.get(app), hits.get(app)) else {
            continue;
        };
        for cap in device_capabilities(model, &products, product, *ia, app_hits) {
            let rank = match (cap.status.as_str(), cap.active) {
                ("provides", _) => 0,
                (_, false) => 1,
                _ => 2,
            };
            out.push((rank, cap));
        }
    }
    out.sort_by_key(|(rank, _)| *rank);
    out.into_iter().take(limit).map(|(_, c)| c).collect()
}

/// The matches on one device, grouped by the page or channel holding them.
fn device_capabilities(
    model: &Model,
    products: &ProductModels,
    product: &ProductModel,
    ia: IndividualAddress,
    hits: &[Hit<'_>],
) -> Vec<Capability> {
    let Some(loaded) = model.devices.get(&ia) else {
        return Vec::new();
    };
    let device = &loaded.device;
    let Ok(view) = device_view(model, products, ia, None) else {
        return Vec::new();
    };
    // The device's shown parameters: ref -> (file key, stored value).
    let mut shown: HashMap<String, (String, Option<String>)> = HashMap::new();
    for (reference, p) in &device.lock.parameters {
        shown.insert(reference.clone(), (p.key.clone(), None));
    }
    for (mem_key, value) in &device.parameters {
        if crate::files::is_hidden_mem_key(mem_key) {
            continue;
        }
        let Some((_, reference)) = mem_key.split_once('@') else {
            continue;
        };
        let entry = shown
            .entry(reference.to_string())
            .or_insert_with(|| (mem_key.clone(), None));
        entry.1 = Some(value.clone());
    }
    // Parameter id -> the ref the lock lists for it.
    let shown_params: HashMap<&str, &str> = device
        .lock
        .parameters
        .iter()
        .filter_map(|(r, p)| Some((p.param.as_deref()?, r.as_str())))
        .collect();

    let rows: Vec<&BlockRow> = view.blocks.iter().chain(&view.inactive_channels).collect();
    let mut row_of: HashMap<&str, &BlockRow> = HashMap::new();
    for row in &rows {
        row_of.entry(row.id.as_str()).or_insert(row);
        for id in &row.includes {
            row_of.entry(id.as_str()).or_insert(row);
        }
    }
    let parent_of: HashMap<&str, Option<&str>> = product
        .blocks()
        .iter()
        .map(|b| (b.id.as_str(), b.parent.as_deref()))
        .collect();
    let title_of: HashMap<&str, Option<&str>> = product
        .blocks()
        .iter()
        .map(|b| (b.id.as_str(), b.text.as_deref()))
        .collect();
    let row_for = |block: &str| -> Option<&BlockRow> {
        let mut at = Some(block);
        while let Some(id) = at {
            if let Some(row) = row_of.get(id) {
                return Some(*row);
            }
            at = parent_of.get(id).copied().flatten();
        }
        None
    };

    // (label, handle) -> the capability being built.
    let mut groups: Vec<Capability> = Vec::new();
    for hit in hits {
        let row = hit.block.and_then(row_for);
        let shown_entry = shown.get(hit.reference).or_else(|| {
            shown_params
                .get(ref_param_id(hit.reference))
                .and_then(|r| shown.get(*r))
        });
        let active = match row {
            Some(row) => row.active,
            None => shown_entry.is_some(),
        };
        let label = match (row, hit.block) {
            (Some(row), _) => row.text.clone(),
            (None, Some(b)) => title_of.get(b).copied().flatten().map(str::to_string),
            (None, None) => shown_params
                .get(ref_param_id(hit.reference))
                .and_then(|r| device.lock.parameters.get(*r))
                .and_then(|p| p.channel.as_deref())
                .map(|c| device.channel_handle(c)),
        };
        let current: Option<i64> = shown_entry
            .and_then(|(_, v)| v.clone())
            .or_else(|| {
                // A parameter no page holds stands for its one ref, if one.
                let reference = match (hit.block, hit.def.refs.as_slice()) {
                    (None, [only]) => only.as_str(),
                    _ => hit.reference,
                };
                product.ref_default(reference).map(str::to_string)
            })
            .and_then(|v| {
                let v = v.trim();
                v.parse::<i64>()
                    .ok()
                    .or_else(|| crate::param_model::enum_code(&hit.def.labels, v))
            });
        let key = shown_entry.map(|(k, _)| k.clone()).filter(|_| active);
        let parameter = hit.text.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut matches: Vec<Match> = hit
            .choices
            .iter()
            .map(|(code, choice)| Match {
                parameter: parameter.clone(),
                key: key.clone(),
                choice: Some(choice.to_string()),
                selected: active && shown_entry.is_some() && current == Some(*code),
            })
            .collect();
        if hit.plain {
            matches.push(Match {
                parameter: parameter.clone(),
                key: key.clone(),
                choice: None,
                selected: active && shown_entry.is_some(),
            });
        }
        let handle = row.map(|r| r.handle.clone());
        let group = match groups
            .iter_mut()
            .find(|g| g.block_or_channel == label && g.handle == handle)
        {
            Some(g) => g,
            None => {
                let inactive = row.and_then(|r| r.inactive.as_ref());
                groups.push(Capability {
                    device_ia: ia.to_string(),
                    device_name: device.name.clone(),
                    block_or_channel: label.clone(),
                    handle: handle.clone(),
                    parameter: String::new(),
                    key: None,
                    choice: None,
                    active,
                    selected: false,
                    status: String::new(),
                    more: Vec::new(),
                    enable: inactive.map(|i| i.enable.clone()).unwrap_or_default(),
                    needs: inactive.map(|i| i.needs.clone()),
                });
                let last = groups.len() - 1;
                &mut groups[last]
            }
        };
        group.active |= active;
        group.more.extend(matches);
    }
    for g in &mut groups {
        // The best match leads: a selected one, then a choice.
        let mut all = std::mem::take(&mut g.more);
        let best = all
            .iter()
            .position(|m| m.selected)
            .or_else(|| all.iter().position(|m| m.choice.is_some()))
            .unwrap_or(0);
        if best < all.len() {
            let lead = all.remove(best);
            g.parameter = lead.parameter;
            g.key = lead.key;
            g.choice = lead.choice;
            g.selected = lead.selected;
        }
        all.truncate(MORE);
        g.more = all;
        g.status = if g.active && g.selected {
            "provides".to_string()
        } else {
            "could provide".to_string()
        };
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fold_spells_out_umlauts() {
        assert_eq!(fold("Dämmerung"), "daemmerung");
        assert_eq!(fold("Größe"), "groesse");
    }
}
