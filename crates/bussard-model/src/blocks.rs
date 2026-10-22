//! The ETS parameter pages of an application program (issue #298): the
//! `<ParameterBlock>` and `<Channel>` containers of its Dynamic section, as the
//! product model (`.bussard/models/<app>.yaml`, `blocks:`) records them, and
//! which of them a device's configuration shows.
//!
//! The importer writes one [`BlockDef`] per container of the application's own
//! Dynamic section, in document order (the order ETS lists the pages in): its
//! title, the enclosing block, the `<choose>`/`<when>` conditions between that
//! enclosing container and the block ([`Gate`]), the parameter refs it holds
//! on any branch, and the com-objects it shows with the vendor defaults.
//! Containers inside a `<ModuleDef>` are not recorded.
//!
//! [`block_states`] decides per block whether a device shows it: a block is
//! active when the lock lists a parameter or com-object only that block holds,
//! or when its enclosing block is active and every condition holds for the
//! device's values. An inactive block names the conditions that do not hold:
//! the parameters to set to show it.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

/// What kind of Dynamic container a [`BlockDef`] is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    /// A `<ParameterBlock>`: one ETS parameter page or inline group.
    #[default]
    Block,
    /// A `<Channel>`.
    Channel,
}

impl BlockKind {
    fn is_block(&self) -> bool {
        *self == BlockKind::Block
    }
}

/// One `<choose ParamRefId>`/`<when>` condition on the way to a block.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gate {
    /// The controlling parameter ref, app-relative (`P-2238_R-2238`).
    #[serde(rename = "ref")]
    pub reference: String,
    /// The `<when>` test as written: values (`1`, `1 2`), a comparison
    /// (`!=0`, `<5`), or `default` for a `<when default="true">` branch.
    pub test: String,
    /// For a `default` branch, the tests of its sibling branches: it is taken
    /// when none of them matches.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub others: Vec<String>,
}

/// Whether a `<when>` test (not `default`) matches `value`.
fn test_matches(test: &str, value: f64) -> bool {
    let t = test.trim();
    for (prefix, cmp) in [
        ("!=", (|v: f64, x: f64| v != x) as fn(f64, f64) -> bool),
        ("<=", |v, x| v <= x),
        (">=", |v, x| v >= x),
        ("<", |v, x| v < x),
        (">", |v, x| v > x),
    ] {
        if let Some(rest) = t.strip_prefix(prefix) {
            return rest.trim().parse::<f64>().is_ok_and(|x| cmp(value, x));
        }
    }
    value.fract() == 0.0
        && t.split_whitespace()
            .filter_map(|v| v.parse::<i64>().ok())
            .any(|x| x as f64 == value)
}

impl Gate {
    /// Whether the condition holds when its parameter has `value` (`None`:
    /// unknown, which only a `default` branch without siblings passes).
    pub fn holds(&self, value: Option<f64>) -> bool {
        if self.test.trim() == "default" {
            return match value {
                Some(v) => !self.others.iter().any(|o| test_matches(o, v)),
                None => self.others.is_empty(),
            };
        }
        value.is_some_and(|v| test_matches(&self.test, v))
    }

    /// The enum codes (out of `codes`) that make the condition hold, in
    /// order: what to set the parameter to.
    pub fn satisfying(&self, codes: &[i64]) -> Vec<i64> {
        codes
            .iter()
            .copied()
            .filter(|c| self.holds(Some(*c as f64)))
            .collect()
    }

    /// The first exact value the test names (for a parameter without enum
    /// codes to choose from).
    pub fn first_value(&self) -> Option<i64> {
        let t = self.test.trim();
        if t == "default" || t.starts_with(['!', '<', '>']) {
            return None;
        }
        t.split_whitespace().find_map(|v| v.parse().ok())
    }
}

/// One ETS parameter page (`<ParameterBlock>`) or `<Channel>` of an
/// application's Dynamic section.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockDef {
    /// The element id, app-relative (`PB-7243`, `CH-0`).
    pub id: String,
    /// Block or channel.
    #[serde(default, skip_serializing_if = "BlockKind::is_block")]
    pub kind: BlockKind,
    /// The title ETS shows: the `Text`, else the text of the parameter ref
    /// that titles it, else its `Name`; placeholders removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The `Name` attribute, when it differs from the title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// A channel's `Number`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u32>,
    /// The id of the enclosing block or channel, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// The id of the enclosing `<Channel>`, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    /// The conditions between the enclosing container and this one,
    /// outermost first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when: Vec<Gate>,
    /// The parameter refs directly inside (not in a nested block), on any
    /// branch, app-relative, in document order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<String>,
    /// The com-object refs directly inside that the vendor defaults show
    /// (`O-22_R-7`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objects: Vec<String>,
    /// The com-object refs directly inside on other branches only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub other_objects: Vec<String>,
}

/// The com-object number of a com-object ref (`O-22_R-7` → 22,
/// `MD-1_O-3_R-4` → 3).
pub fn object_number(reference: &str) -> Option<u16> {
    let start = reference.rfind("O-")? + 2;
    let digits: String = reference[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// The parameter id of an app-level parameter ref (`P-2238_R-2238` →
/// `P-2238`).
pub fn ref_param_id(reference: &str) -> &str {
    reference
        .rsplit_once("_R-")
        .map_or(reference, |(head, _)| head)
}

/// Whether a device shows a block, and if not, why.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BlockState {
    /// The configuration shows the block.
    pub active: bool,
    /// For an inactive block whose enclosing block is active: the indexes
    /// into [`BlockDef::when`] of the conditions that do not hold.
    pub failing: Vec<usize>,
    /// The block is inactive because its enclosing block is.
    pub parent_inactive: bool,
}

/// The index of each block's parent in `blocks` (the first block with that
/// id that precedes it).
pub fn parent_indexes(blocks: &[BlockDef]) -> Vec<Option<usize>> {
    let mut first: HashMap<&str, usize> = HashMap::new();
    let mut out = Vec::with_capacity(blocks.len());
    for (i, b) in blocks.iter().enumerate() {
        out.push(b.parent.as_deref().and_then(|p| first.get(p).copied()));
        first.entry(b.id.as_str()).or_insert(i);
    }
    out
}

/// Decides for every block of `blocks` whether a device shows it.
///
/// `active_params` are the parameter refs the lock lists for the device,
/// `active_objects` its com-object refs, and `value_of` the device's value
/// of a parameter ref (stored, else the vendor default) as a number.
pub fn block_states(
    blocks: &[BlockDef],
    active_params: &HashSet<&str>,
    active_objects: &HashSet<&str>,
    value_of: &dyn Fn(&str) -> Option<f64>,
) -> Vec<BlockState> {
    let parents = parent_indexes(blocks);
    // A ref only one block holds is evidence for that block; one several
    // blocks share (alternative pages showing the same parameter) is not.
    let mut holders: HashMap<&str, usize> = HashMap::new();
    for b in blocks {
        let mut own: HashSet<&str> = HashSet::new();
        for r in b
            .parameters
            .iter()
            .chain(&b.objects)
            .chain(&b.other_objects)
        {
            if own.insert(r) {
                *holders.entry(r).or_default() += 1;
            }
        }
    }
    let mut evidence: Vec<bool> = blocks
        .iter()
        .map(|b| {
            b.parameters
                .iter()
                .any(|r| holders.get(r.as_str()) == Some(&1) && active_params.contains(r.as_str()))
                || b.objects.iter().chain(&b.other_objects).any(|r| {
                    holders.get(r.as_str()) == Some(&1) && active_objects.contains(r.as_str())
                })
        })
        .collect();
    // Children come after their parent: propagate evidence upwards.
    for i in (0..blocks.len()).rev() {
        if evidence[i]
            && let Some(p) = parents[i]
        {
            evidence[p] = true;
        }
    }
    let mut states: Vec<BlockState> = Vec::with_capacity(blocks.len());
    for (i, b) in blocks.iter().enumerate() {
        let parent_active = parents[i].is_none_or(|p| states.get(p).is_none_or(|s| s.active));
        let failing: Vec<usize> = b
            .when
            .iter()
            .enumerate()
            .filter(|(_, g)| !g.holds(value_of(&g.reference)))
            .map(|(n, _)| n)
            .collect();
        let active = evidence[i] || (parent_active && failing.is_empty());
        states.push(BlockState {
            active,
            failing: if active || !parent_active {
                Vec::new()
            } else {
                failing
            },
            parent_inactive: !active && !parent_active,
        });
    }
    states
}

/// The indexes of `index` and every block nested in it, in order.
pub fn subtree(blocks: &[BlockDef], parents: &[Option<usize>], index: usize) -> Vec<usize> {
    let mut inside: Vec<bool> = vec![false; blocks.len()];
    let mut out = Vec::new();
    for i in index..blocks.len() {
        let member = i == index || parents[i].is_some_and(|p| inside[p]);
        if member {
            inside[i] = true;
            out.push(i);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(id: &str, parent: Option<&str>, when: &[(&str, &str)], params: &[&str]) -> BlockDef {
        BlockDef {
            id: id.to_string(),
            parent: parent.map(str::to_string),
            when: when
                .iter()
                .map(|(r, t)| Gate {
                    reference: r.to_string(),
                    test: t.to_string(),
                    others: Vec::new(),
                })
                .collect(),
            parameters: params.iter().map(|p| p.to_string()).collect(),
            ..BlockDef::default()
        }
    }

    #[test]
    fn test_gate_holds_values_compare_and_default() {
        let g = |t: &str, others: &[&str]| Gate {
            reference: "P-1_R-1".into(),
            test: t.into(),
            others: others.iter().map(|o| o.to_string()).collect(),
        };
        assert!(g("1", &[]).holds(Some(1.0)));
        assert!(!g("1", &[]).holds(Some(0.0)));
        assert!(g("1 2", &[]).holds(Some(2.0)));
        assert!(g("!=0", &[]).holds(Some(3.0)));
        assert!(g("<5", &[]).holds(Some(4.0)));
        assert!(!g("<5", &[]).holds(None));
        assert!(g("default", &["1"]).holds(Some(0.0)));
        assert!(!g("default", &["1"]).holds(Some(1.0)));
        assert!(g("default", &[]).holds(None));
        assert_eq!(g("1", &[]).satisfying(&[0, 1]), vec![1]);
        assert_eq!(g("1 2", &[]).first_value(), Some(1));
    }

    #[test]
    fn test_block_states_gates_evidence_and_parents() {
        let blocks = vec![
            block("PB-1", None, &[("P-1_R-1", "1")], &["P-2_R-2"]),
            block("PB-2", Some("PB-1"), &[], &["P-3_R-3"]),
            block("PB-3", None, &[("P-4_R-4", "1")], &["P-5_R-5"]),
        ];
        let params: HashSet<&str> = HashSet::new();
        let objects: HashSet<&str> = HashSet::new();
        let value = |r: &str| match r {
            "P-1_R-1" => Some(0.0),
            "P-4_R-4" => Some(1.0),
            _ => None,
        };
        let s = block_states(&blocks, &params, &objects, &value);
        assert!(!s[0].active);
        assert_eq!(s[0].failing, vec![0]);
        assert!(!s[1].active);
        assert!(s[1].parent_inactive);
        assert!(s[2].active);

        // The lock listing a parameter only PB-2 holds makes PB-2 and PB-1
        // active whatever the values say.
        let params: HashSet<&str> = ["P-3_R-3"].into_iter().collect();
        let s = block_states(&blocks, &params, &objects, &value);
        assert!(s[0].active && s[1].active);
    }

    #[test]
    fn test_object_number_and_param_id() {
        assert_eq!(object_number("O-22_R-7"), Some(22));
        assert_eq!(object_number("MD-1_O-3_R-4"), Some(3));
        assert_eq!(ref_param_id("P-2238_R-2238"), "P-2238");
    }

    #[test]
    fn test_subtree_follows_nesting() {
        let blocks = vec![
            block("PB-1", None, &[], &[]),
            block("PB-2", Some("PB-1"), &[], &[]),
            block("PB-3", Some("PB-2"), &[], &[]),
            block("PB-4", None, &[], &[]),
        ];
        let parents = parent_indexes(&blocks);
        assert_eq!(subtree(&blocks, &parents, 0), vec![0, 1, 2]);
        assert_eq!(subtree(&blocks, &parents, 3), vec![3]);
    }
}
