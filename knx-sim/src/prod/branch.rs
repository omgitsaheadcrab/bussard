//! Which `<choose>`/`<when>` branch of a load procedure the product's own
//! vendor defaults select.
//!
//! A load procedure may guard a preflight compare with a parameter: the MDT
//! central operation unit (`M-0083_A-0054-14-0D63`, bussard issue #400)
//! compares object 0 PID 19 (`PID_MANUFACTURER_DATA`) with `00000000` when its
//! "Hardware revision" parameter is "to R1.9" and with `01000000` when it is
//! "from R2.0". The device reports its revision; the parameter only says which
//! one the project expects. The product's own default for that parameter is
//! the hardware the vendor ships against this application, so the simulated
//! device holds the value the default branch compares, not the first compare
//! the file happens to list.

use std::collections::HashMap;

/// One `<when>` a step sits under: the parameter ref its `<choose>` tests and
/// the condition of the branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Guard {
    /// The `ParamRefId` of the enclosing `<choose>`.
    pub param_ref: String,
    /// The branch condition.
    pub test: WhenTest,
}

/// Compare candidates in document order: a key (the property), the value the
/// compare expects, and the guards it sits under.
pub(crate) type Candidates<K> = Vec<(K, Vec<u8>, Vec<Guard>)>;

/// The condition of a `<when>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WhenTest {
    /// `test="…"`: a space-separated list of values, each optionally prefixed
    /// with a relational operator (`<`, `<=`, `>`, `>=`, `!=`, `=`).
    Test(Vec<String>),
    /// `default="true"`: holds when no sibling `<when>` before it holds.
    Default(Vec<Vec<String>>),
}

/// The `<choose>` frames open at the current parse position.
#[derive(Debug, Default)]
pub(crate) struct ChooseStack {
    /// Each open `<choose>`: its parameter ref and the `test` token lists of
    /// the `<when>`s seen so far inside it.
    chooses: Vec<(String, Vec<Vec<String>>)>,
    /// The guards of the open `<when>`s, outermost first.
    guards: Vec<Guard>,
}

impl ChooseStack {
    /// A `<choose ParamRefId="…">` start tag.
    pub fn open_choose(&mut self, param_ref: String) {
        self.chooses.push((param_ref, Vec::new()));
    }

    /// A `</choose>` end tag.
    pub fn close_choose(&mut self) {
        self.chooses.pop();
    }

    /// A `<when>` tag. With `has_children` its guard stays in force until
    /// [`ChooseStack::close_when`]; a self-closing one only counts as a
    /// sibling for a later `default` branch.
    pub fn open_when(&mut self, test: Option<&str>, default: bool, has_children: bool) {
        let Some((param_ref, seen)) = self.chooses.last_mut() else {
            return;
        };
        let tokens: Vec<String> = test
            .map(|t| t.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default();
        let guard = Guard {
            param_ref: param_ref.clone(),
            test: if default {
                WhenTest::Default(seen.clone())
            } else {
                WhenTest::Test(tokens.clone())
            },
        };
        if !default {
            seen.push(tokens);
        }
        if has_children {
            self.guards.push(guard);
        }
    }

    /// A `</when>` end tag.
    pub fn close_when(&mut self) {
        self.guards.pop();
    }

    /// The guards in force at the current position.
    pub fn guards(&self) -> Vec<Guard> {
        self.guards.clone()
    }
}

/// The vendor default value of every parameter ref of the application: the
/// `ParameterRef`'s own `Value`, else its `Parameter`'s.
#[derive(Debug, Default)]
pub(crate) struct Defaults {
    /// `Parameter` id to its `Value`.
    parameters: HashMap<String, String>,
    /// `ParameterRef` id to its `RefId` and optional `Value`.
    refs: HashMap<String, (String, Option<String>)>,
}

impl Defaults {
    /// Record a `<Parameter Id Value>`.
    pub fn parameter(&mut self, id: String, value: Option<String>) {
        if let Some(value) = value {
            self.parameters.insert(id, value);
        }
    }

    /// Record a `<ParameterRef Id RefId Value>`.
    pub fn parameter_ref(&mut self, id: String, ref_id: String, value: Option<String>) {
        self.refs.insert(id, (ref_id, value));
    }

    /// The default value of a parameter ref, if the product declares one.
    fn value(&self, param_ref: &str) -> Option<&str> {
        let (ref_id, value) = self.refs.get(param_ref)?;
        value
            .as_deref()
            .or_else(|| self.parameters.get(ref_id).map(String::as_str))
    }

    /// Whether every guard holds under the vendor defaults. A guard on a
    /// parameter ref without a known default does not hold.
    pub fn holds(&self, guards: &[Guard]) -> bool {
        guards.iter().all(|g| {
            let Some(value) = self.value(&g.param_ref) else {
                return false;
            };
            match &g.test {
                WhenTest::Test(tokens) => tokens_match(tokens, value),
                WhenTest::Default(siblings) => !siblings.iter().any(|t| tokens_match(t, value)),
            }
        })
    }

    /// Picks one value per key from compare candidates in document order: the
    /// first whose branch the vendor defaults select, else the first listed.
    /// Keys keep the order of their first appearance.
    pub fn pick<K: PartialEq + Clone>(
        &self,
        candidates: &[(K, Vec<u8>, Vec<Guard>)],
    ) -> Vec<(K, Vec<u8>)> {
        let mut out: Vec<(K, Vec<u8>)> = Vec::new();
        for (key, _, _) in candidates {
            if out.iter().any(|(k, _)| k == key) {
                continue;
            }
            let mut same = candidates.iter().filter(|(k, _, _)| k == key);
            let chosen = same
                .clone()
                .find(|(_, _, guards)| self.holds(guards))
                .or_else(|| same.next());
            if let Some((_, value, _)) = chosen {
                out.push((key.clone(), value.clone()));
            }
        }
        out
    }
}

/// Whether any token of a `test` list matches `value`.
fn tokens_match(tokens: &[String], value: &str) -> bool {
    tokens.iter().any(|t| token_matches(t, value))
}

/// One `test` token against a value: numeric when both sides are numbers,
/// else exact text equality (only for `=`/no operator).
fn token_matches(token: &str, value: &str) -> bool {
    let (op, operand) = ["<=", ">=", "!=", "<", ">", "="]
        .iter()
        .find_map(|op| token.strip_prefix(op).map(|rest| (*op, rest)))
        .unwrap_or(("=", token));
    match (value.trim().parse::<f64>(), operand.trim().parse::<f64>()) {
        (Ok(v), Ok(o)) => match op {
            "<=" => v <= o,
            ">=" => v >= o,
            "!=" => v != o,
            "<" => v < o,
            ">" => v > o,
            _ => v == o,
        },
        _ => match op {
            "!=" => value != operand,
            "=" => value == operand,
            _ => false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard(param_ref: &str, test: &[&str]) -> Guard {
        Guard {
            param_ref: param_ref.to_string(),
            test: WhenTest::Test(test.iter().map(|s| s.to_string()).collect()),
        }
    }

    #[test]
    fn test_token_matches_lists_and_operators() {
        assert!(token_matches("1", "1"));
        assert!(!token_matches("0", "1"));
        assert!(token_matches("<2", "1"));
        assert!(!token_matches(">=2", "1"));
        assert!(token_matches("!=0", "1"));
        assert!(token_matches("abc", "abc"));
        assert!(!token_matches("<abc", "abc"));
    }

    #[test]
    fn test_holds_uses_ref_value_over_parameter_value() {
        let mut d = Defaults::default();
        d.parameter("P-1".into(), Some("0".into()));
        d.parameter_ref("P-1_R-1".into(), "P-1".into(), None);
        d.parameter_ref("P-1_R-2".into(), "P-1".into(), Some("3".into()));
        assert!(d.holds(&[guard("P-1_R-1", &["0"])]));
        assert!(d.holds(&[guard("P-1_R-2", &["2", "3"])]));
        assert!(!d.holds(&[guard("P-1_R-2", &["0"])]));
        // An unknown ref never holds.
        assert!(!d.holds(&[guard("P-9_R-9", &["0"])]));
    }

    #[test]
    fn test_pick_prefers_the_default_branch_and_falls_back_to_the_first() {
        let mut d = Defaults::default();
        d.parameter("P-1".into(), Some("1".into()));
        d.parameter_ref("R".into(), "P-1".into(), None);
        let candidates = vec![
            (19u8, vec![0x00], vec![guard("R", &["0"])]),
            (19u8, vec![0x01], vec![guard("R", &["1"])]),
            (12u8, vec![0xAA], vec![guard("R", &["7"])]),
        ];
        assert_eq!(
            d.pick(&candidates),
            vec![(19, vec![0x01]), (12, vec![0xAA])]
        );
    }

    #[test]
    fn test_choose_stack_default_branch_negates_its_siblings() {
        let mut d = Defaults::default();
        d.parameter("P".into(), Some("5".into()));
        d.parameter_ref("R".into(), "P".into(), None);
        let mut s = ChooseStack::default();
        s.open_choose("R".into());
        s.open_when(Some("1 2"), false, false);
        s.open_when(None, true, true);
        assert!(d.holds(&s.guards()));
        s.close_when();
        assert!(s.guards().is_empty());
        s.close_choose();
    }
}
