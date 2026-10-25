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
//! Which of them a device's configuration shows is the Dynamic evaluator's
//! answer ([`crate::dynamic::evaluate_dynamic`], issue #345), not this
//! module's: [`Gate`] is only the words a page condition is written in.

use std::collections::{BTreeMap, HashMap};

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
    /// For each of [`Self::parameters`] that sits under a `<choose>` inside
    /// this block on one branch only: the conditions between the block and
    /// the ref, outermost first (issue #315). A ref outside any `<choose>`,
    /// or on several branches, has no entry.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parameter_when: BTreeMap<String, Vec<Gate>>,
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
