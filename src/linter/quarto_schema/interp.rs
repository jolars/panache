//! Recursive validator over a distilled [`QuartoSchema`].
//!
//! The interpreter walks a [`SchemaValue`] (a YAML value abstracted to
//! type-plus-span; see the bridge in [`super::value`]) against a schema node and
//! reports [`SchemaError`]s. Design points that matter for low-noise output:
//!
//! - **Open vs closed.** Quarto's top-level frontmatter object is *open* (it
//!   must allow pandoc passthrough and arbitrary metadata), so unknown keys
//!   there are only flagged when they are a near-miss typo of a known key
//!   (did-you-mean). The 157 `closed: true` objects (e.g. a specific format's
//!   option block) reject unknown keys outright.
//! - **`allOf` of objects is merged**, not checked branch-by-branch: Quarto
//!   composes the document schema as `allOf[object, object, ref, ref]`, so a key
//!   declared in any branch is known, and closure is the OR of the branches.
//! - **`anyOf` reports the best branch.** A value passes if it matches any
//!   branch; if none match, we surface the single closest branch's errors rather
//!   than the union, so `format: html` does not also complain "expected null".

use std::collections::HashMap;

use rowan::TextRange;

use super::model::{PatternProp, QuartoSchema, SchemaNode};
use crate::linter::fuzzy::nearest_match;

/// Resolved scalar type per the YAML 1.2 core schema (what js-yaml—and thus
/// Quarto—infers for a plain scalar).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarType {
    String,
    Int,
    Float,
    Bool,
    Null,
}

/// A YAML value abstracted for schema validation, carrying CST spans so
/// diagnostics land tightly.
#[derive(Debug, Clone)]
pub struct SchemaValue {
    pub span: TextRange,
    pub kind: ValueKind,
}

#[derive(Debug, Clone)]
pub enum ValueKind {
    /// A scalar with its resolved YAML 1.2 type and its cooked literal text
    /// (the latter is needed to check `enum` membership).
    Scalar {
        ty: ScalarType,
        literal: String,
    },
    Map(Vec<MapEntry>),
    Seq(Vec<SchemaValue>),
}

#[derive(Debug, Clone)]
pub struct MapEntry {
    pub key: String,
    pub key_span: TextRange,
    pub value: SchemaValue,
}

/// A schema violation, located by CST span. Message and severity are the rule's
/// concern; this stays about *what* was wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaError {
    pub span: TextRange,
    pub kind: ErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    /// A map key the schema does not declare. `closed` distinguishes "not
    /// allowed here" (closed object) from a soft did-you-mean on an open object.
    UnknownKey {
        key: String,
        suggestion: Option<String>,
        closed: bool,
    },
    /// The value's type does not match the schema. `expected` is a human phrase
    /// ("a boolean", "an object").
    TypeMismatch { expected: String },
    /// A scalar that is not one of an `enum`'s allowed values.
    InvalidEnum { allowed: Vec<String> },
}

/// Validate `value` against the definition `root_id` in `schema`.
///
/// Returns an empty vector when `root_id` is unknown (nothing to validate
/// against) or the value conforms.
pub fn validate(schema: &QuartoSchema, root_id: &str, value: &SchemaValue) -> Vec<SchemaError> {
    let Some(root) = schema.defs.get(root_id) else {
        return Vec::new();
    };
    let ctx = Ctx { schema };
    ctx.check(root, value, &mut Vec::new())
}

struct Ctx<'s> {
    schema: &'s QuartoSchema,
}

/// Guards against runaway recursion through `ref` cycles and deeply nested
/// schemas. Quarto's schema is recursive (e.g. navigation items), so both a
/// visited-ref set and a hard depth cap are needed.
const MAX_DEPTH: usize = 64;

impl<'s> Ctx<'s> {
    /// Errors from validating `value` against `node`. Empty means it conforms.
    fn check(
        &self,
        node: &'s SchemaNode,
        value: &SchemaValue,
        seen: &mut Vec<&'s str>,
    ) -> Vec<SchemaError> {
        if seen.len() > MAX_DEPTH {
            return Vec::new();
        }
        match node {
            SchemaNode::Any => Vec::new(),
            SchemaNode::Ref { id } => {
                if seen.contains(&id.as_str()) {
                    return Vec::new();
                }
                let Some(target) = self.schema.defs.get(id) else {
                    return Vec::new();
                };
                seen.push(id);
                let errors = self.check(target, value, seen);
                seen.pop();
                errors
            }
            SchemaNode::AnyOf { of } => self.check_any_of(of, value, seen),
            SchemaNode::AllOf { of } => self.check_all_of(of, value, seen),
            SchemaNode::Object { .. } => self.check_object(node, value, seen),
            SchemaNode::Array { items } => self.check_array(items.as_deref(), value, seen),
            SchemaNode::Enum { values } => self.check_enum(values, value),
            SchemaNode::String | SchemaNode::Number | SchemaNode::Boolean | SchemaNode::Null => {
                self.check_scalar_type(node, value)
            }
        }
    }

    /// A value passes an `anyOf` if it matches any branch. Otherwise surface the
    /// single best-matching branch's errors (fewest errors, preferring branches
    /// whose shape matches the value), so the message is the most relevant one.
    fn check_any_of(
        &self,
        branches: &'s [SchemaNode],
        value: &SchemaValue,
        seen: &mut Vec<&'s str>,
    ) -> Vec<SchemaError> {
        // Track the best branch as (is-shape-match, errors). A branch whose
        // top-level shape matches the value (object node for a map, etc.) always
        // beats a non-matching branch, regardless of error count — otherwise a
        // map with several errors loses to the `null` branch's single "expected
        // null", reporting a misleading whole-block error.
        let mut best: Option<(bool, Vec<SchemaError>)> = None;
        for branch in branches {
            let errors = self.check(branch, value, seen);
            if errors.is_empty() {
                return Vec::new();
            }
            let new_shape = shape_matches(self.schema, branch, value);
            let better = match &best {
                None => true,
                Some((best_shape, best_errors)) => {
                    if new_shape != *best_shape {
                        new_shape
                    } else {
                        errors.len() < best_errors.len()
                    }
                }
            };
            if better {
                best = Some((new_shape, errors));
            }
        }
        // No branch matched. For a scalar value whose alternatives are all
        // scalar/enum (e.g. the common `anyOf[boolean, enum]`), report one
        // merged expectation ("a boolean or one of: fenced") rather than a
        // single branch's partial complaint.
        if matches!(value.kind, ValueKind::Scalar { .. }) {
            let mut alts = Vec::new();
            if self.collect_scalar_alts(branches, &mut Vec::new(), &mut alts) && !alts.is_empty() {
                alts.dedup();
                return vec![SchemaError {
                    span: value.span,
                    kind: ErrorKind::TypeMismatch {
                        expected: alts.join(" or "),
                    },
                }];
            }
        }
        best.map(|(_, errors)| errors).unwrap_or_default()
    }

    /// Collect the scalar/enum expectations of every branch, returning `false`
    /// if any branch is a container (object/array) or otherwise not cleanly
    /// describable as a scalar alternative.
    fn collect_scalar_alts(
        &self,
        branches: &'s [SchemaNode],
        seen: &mut Vec<&'s str>,
        out: &mut Vec<String>,
    ) -> bool {
        branches
            .iter()
            .all(|b| self.collect_scalar_alt(b, seen, out))
    }

    fn collect_scalar_alt(
        &self,
        node: &'s SchemaNode,
        seen: &mut Vec<&'s str>,
        out: &mut Vec<String>,
    ) -> bool {
        match node {
            SchemaNode::String => out.push("a string".to_string()),
            SchemaNode::Boolean => out.push("a boolean".to_string()),
            SchemaNode::Number => out.push("a number".to_string()),
            SchemaNode::Null => out.push("null".to_string()),
            SchemaNode::Enum { values } => out.push(enum_expected(values)),
            // A permissive branch would have matched already, so it never
            // reaches here; treat as describable without adding a phrase.
            SchemaNode::Any => {}
            SchemaNode::AnyOf { of } => return self.collect_scalar_alts(of, seen, out),
            SchemaNode::Ref { id } => {
                if seen.contains(&id.as_str()) {
                    return true;
                }
                let Some(target) = self.schema.defs.get(id) else {
                    return true;
                };
                seen.push(id);
                let ok = self.collect_scalar_alt(target, seen, out);
                seen.pop();
                return ok;
            }
            // Containers and allOf are not cleanly mergeable as scalar choices.
            _ => return false,
        }
        true
    }

    /// For a map value, merge all object branches and validate once. For other
    /// values, every branch must hold, so concatenate their errors.
    fn check_all_of(
        &self,
        branches: &'s [SchemaNode],
        value: &SchemaValue,
        seen: &mut Vec<&'s str>,
    ) -> Vec<SchemaError> {
        if let ValueKind::Map(entries) = &value.kind {
            let mut view = ObjView::default();
            for branch in branches {
                self.collect_object(branch, &mut view, seen);
            }
            return self.check_map(&view, entries, seen);
        }
        let mut errors = Vec::new();
        for branch in branches {
            errors.extend(self.check(branch, value, seen));
        }
        errors
    }

    fn check_object(
        &self,
        node: &'s SchemaNode,
        value: &SchemaValue,
        seen: &mut Vec<&'s str>,
    ) -> Vec<SchemaError> {
        let ValueKind::Map(entries) = &value.kind else {
            return vec![SchemaError {
                span: value.span,
                kind: ErrorKind::TypeMismatch {
                    expected: "an object".to_string(),
                },
            }];
        };
        let mut view = ObjView::default();
        self.collect_object(node, &mut view, seen);
        self.check_map(&view, entries, seen)
    }

    /// Flatten an object/`allOf`/`ref` chain into a single merged view: union of
    /// properties and patterns, closed if any contributing object is closed.
    fn collect_object(
        &self,
        node: &'s SchemaNode,
        view: &mut ObjView<'s>,
        seen: &mut Vec<&'s str>,
    ) {
        match node {
            SchemaNode::Object {
                properties,
                closed,
                pattern,
            } => {
                view.closed |= *closed;
                view.had_object = true;
                for (k, v) in properties {
                    view.props.insert(k.as_str(), v);
                }
                for p in pattern {
                    view.patterns.push(p);
                }
            }
            SchemaNode::AllOf { of } => {
                for b in of {
                    self.collect_object(b, view, seen);
                }
            }
            SchemaNode::Ref { id } => {
                if seen.contains(&id.as_str()) {
                    return;
                }
                if let Some(target) = self.schema.defs.get(id) {
                    seen.push(id);
                    self.collect_object(target, view, seen);
                    seen.pop();
                }
            }
            _ => {}
        }
    }

    fn check_map(
        &self,
        view: &ObjView<'s>,
        entries: &[MapEntry],
        seen: &mut Vec<&'s str>,
    ) -> Vec<SchemaError> {
        let mut errors = Vec::new();
        for entry in entries {
            if let Some(prop) = view.props.get(entry.key.as_str()) {
                errors.extend(self.check(prop, &entry.value, seen));
            } else if let Some(pat) = view.match_pattern(&entry.key) {
                errors.extend(self.check(pat, &entry.value, seen));
            } else {
                // Unknown key. Closed objects reject it outright; open objects
                // only flag a near-miss typo of a known key. Open objects use a
                // tighter edit-distance budget (1) because the candidate set is
                // large (hundreds of keys), so distance-2 matches are noisy.
                let budget = if view.closed { 2 } else { 1 };
                let suggestion = did_you_mean(&entry.key, view.props.keys().copied(), budget);
                if view.closed || suggestion.is_some() {
                    errors.push(SchemaError {
                        span: entry.key_span,
                        kind: ErrorKind::UnknownKey {
                            key: entry.key.clone(),
                            suggestion,
                            closed: view.closed,
                        },
                    });
                }
            }
        }
        errors
    }

    fn check_array(
        &self,
        items: Option<&'s SchemaNode>,
        value: &SchemaValue,
        seen: &mut Vec<&'s str>,
    ) -> Vec<SchemaError> {
        let ValueKind::Seq(values) = &value.kind else {
            return vec![SchemaError {
                span: value.span,
                kind: ErrorKind::TypeMismatch {
                    expected: "an array".to_string(),
                },
            }];
        };
        let Some(items) = items else {
            return Vec::new();
        };
        let mut errors = Vec::new();
        for v in values {
            errors.extend(self.check(items, v, seen));
        }
        errors
    }

    fn check_enum(&self, values: &[serde_json::Value], value: &SchemaValue) -> Vec<SchemaError> {
        let ValueKind::Scalar { literal, .. } = &value.kind else {
            return vec![SchemaError {
                span: value.span,
                kind: ErrorKind::TypeMismatch {
                    expected: enum_expected(values),
                },
            }];
        };
        if values.iter().any(|v| render_json_scalar(v) == *literal) {
            Vec::new()
        } else {
            vec![SchemaError {
                span: value.span,
                kind: ErrorKind::InvalidEnum {
                    allowed: values.iter().map(render_json_scalar).collect(),
                },
            }]
        }
    }

    fn check_scalar_type(&self, node: &SchemaNode, value: &SchemaValue) -> Vec<SchemaError> {
        let ValueKind::Scalar { ty, .. } = &value.kind else {
            return vec![SchemaError {
                span: value.span,
                kind: ErrorKind::TypeMismatch {
                    expected: scalar_expected(node),
                },
            }];
        };
        let ok = match node {
            // A string schema accepts any scalar: YAML authors routinely write
            // unquoted strings, and Quarto coerces. Only container/scalar
            // category mismatches are worth flagging.
            SchemaNode::String => true,
            SchemaNode::Boolean => matches!(ty, ScalarType::Bool),
            SchemaNode::Number => matches!(ty, ScalarType::Int | ScalarType::Float),
            SchemaNode::Null => matches!(ty, ScalarType::Null),
            _ => true,
        };
        if ok {
            Vec::new()
        } else {
            vec![SchemaError {
                span: value.span,
                kind: ErrorKind::TypeMismatch {
                    expected: scalar_expected(node),
                },
            }]
        }
    }
}

/// Merged object constraints gathered across an `allOf`/`ref` chain.
#[derive(Default)]
struct ObjView<'s> {
    props: HashMap<&'s str, &'s SchemaNode>,
    patterns: Vec<&'s PatternProp>,
    closed: bool,
    had_object: bool,
}

impl<'s> ObjView<'s> {
    fn match_pattern(&self, key: &str) -> Option<&'s SchemaNode> {
        for p in &self.patterns {
            if regex_is_match(&p.re, key) {
                return Some(&p.schema);
            }
        }
        None
    }
}

/// Whether `node`'s top-level shape matches `value`'s kind (used to prefer the
/// most relevant `anyOf` branch).
fn shape_matches(schema: &QuartoSchema, node: &SchemaNode, value: &SchemaValue) -> bool {
    match resolve(schema, node) {
        SchemaNode::Object { .. } | SchemaNode::AllOf { .. } => {
            matches!(value.kind, ValueKind::Map(_))
        }
        SchemaNode::Array { .. } => matches!(value.kind, ValueKind::Seq(_)),
        SchemaNode::String
        | SchemaNode::Number
        | SchemaNode::Boolean
        | SchemaNode::Null
        | SchemaNode::Enum { .. } => matches!(value.kind, ValueKind::Scalar { .. }),
        _ => false,
    }
}

/// Resolve a `ref` to its target (one hop is enough for shape inspection).
fn resolve<'s>(schema: &'s QuartoSchema, node: &'s SchemaNode) -> &'s SchemaNode {
    let mut current = node;
    let mut hops = 0;
    while let SchemaNode::Ref { id } = current {
        let Some(target) = schema.defs.get(id) else {
            break;
        };
        current = target;
        hops += 1;
        if hops > MAX_DEPTH {
            break;
        }
    }
    current
}

fn scalar_expected(node: &SchemaNode) -> String {
    match node {
        SchemaNode::String => "a string",
        SchemaNode::Boolean => "a boolean",
        SchemaNode::Number => "a number",
        SchemaNode::Null => "null",
        _ => "a scalar",
    }
    .to_string()
}

fn enum_expected(values: &[serde_json::Value]) -> String {
    format!("one of: {}", render_enum(values))
}

fn render_enum(values: &[serde_json::Value]) -> String {
    values
        .iter()
        .map(render_json_scalar)
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_json_scalar(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Suggest the closest known key to `key`, if one is a plausible typo.
///
/// Conservative on purpose: only short edit distances on reasonably long keys,
/// so legitimate custom keys on *open* objects are not nagged.
fn did_you_mean<'a>(
    key: &str,
    candidates: impl Iterator<Item = &'a str>,
    budget: usize,
) -> Option<String> {
    if key.len() < 3 || budget == 0 {
        return None;
    }
    let max = budget.min(if key.len() <= 5 { 1 } else { 2 });
    nearest_match(key, candidates.filter(|c| c.len() >= 3), max).map(str::to_string)
}

#[cfg(not(target_arch = "wasm32"))]
fn regex_is_match(pattern: &str, text: &str) -> bool {
    use std::sync::RwLock;

    use regex::Regex;
    // Patterns come from a fixed vendored schema, so the compiled-regex cache is
    // bounded by the number of distinct patternProperties (a few hundred).
    static CACHE: RwLock<Option<HashMap<String, Option<Regex>>>> = RwLock::new(None);

    if let Some(map) = CACHE.read().unwrap().as_ref()
        && let Some(entry) = map.get(pattern)
    {
        return entry.as_ref().is_some_and(|re| re.is_match(text));
    }
    let compiled = Regex::new(pattern).ok();
    let result = compiled.as_ref().is_some_and(|re| re.is_match(text));
    let mut guard = CACHE.write().unwrap();
    guard
        .get_or_insert_with(HashMap::new)
        .insert(pattern.to_string(), compiled);
    result
}

#[cfg(target_arch = "wasm32")]
fn regex_is_match(pattern: &str, text: &str) -> bool {
    regex::Regex::new(pattern).is_ok_and(|re| re.is_match(text))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::linter::quarto_schema::model::{PatternProp, QuartoSchema, Roots, SchemaNode};

    fn span() -> TextRange {
        TextRange::new(0.into(), 1.into())
    }

    fn scalar(ty: ScalarType) -> SchemaValue {
        let literal = match ty {
            ScalarType::Bool => "true",
            ScalarType::Null => "null",
            ScalarType::Int => "1",
            ScalarType::Float => "1.0",
            ScalarType::String => "x",
        };
        scalar_lit(ty, literal)
    }

    fn scalar_lit(ty: ScalarType, literal: &str) -> SchemaValue {
        SchemaValue {
            span: span(),
            kind: ValueKind::Scalar {
                ty,
                literal: literal.to_string(),
            },
        }
    }

    fn entry(key: &str, value: SchemaValue) -> MapEntry {
        MapEntry {
            key: key.to_string(),
            key_span: span(),
            value,
        }
    }

    fn map(entries: Vec<MapEntry>) -> SchemaValue {
        SchemaValue {
            span: span(),
            kind: ValueKind::Map(entries),
        }
    }

    fn schema_of(defs: Vec<(&str, SchemaNode)>) -> QuartoSchema {
        QuartoSchema {
            version: "test".to_string(),
            roots: Roots {
                frontmatter: "root".to_string(),
                project: "root".to_string(),
                cell_knitr: None,
                cell_jupyter: None,
            },
            defs: defs
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect::<BTreeMap<_, _>>(),
        }
    }

    fn obj(props: Vec<(&str, SchemaNode)>, closed: bool) -> SchemaNode {
        SchemaNode::Object {
            properties: props
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect::<BTreeMap<_, _>>(),
            closed,
            pattern: Vec::new(),
        }
    }

    #[test]
    fn closed_object_rejects_unknown_key() {
        let s = schema_of(vec![(
            "root",
            obj(vec![("title", SchemaNode::String)], true),
        )]);
        let v = map(vec![entry("ttle", scalar(ScalarType::String))]);
        let errors = validate(&s, "root", &v);
        assert_eq!(errors.len(), 1);
        match &errors[0].kind {
            ErrorKind::UnknownKey {
                key,
                suggestion,
                closed,
            } => {
                assert_eq!(key, "ttle");
                assert_eq!(suggestion.as_deref(), Some("title"));
                assert!(closed);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn open_object_only_flags_near_miss() {
        let s = schema_of(vec![(
            "root",
            obj(vec![("format", SchemaNode::String)], false),
        )]);
        // A near-miss typo IS flagged.
        let typo = map(vec![entry("forrmat", scalar(ScalarType::String))]);
        let errors = validate(&s, "root", &typo);
        assert_eq!(errors.len(), 1);
        assert!(matches!(
            &errors[0].kind,
            ErrorKind::UnknownKey { suggestion: Some(s), closed: false, .. } if s == "format"
        ));
        // A genuinely custom key is NOT flagged on an open object.
        let custom = map(vec![entry("my-custom-meta", scalar(ScalarType::String))]);
        assert!(validate(&s, "root", &custom).is_empty());
    }

    #[test]
    fn type_mismatch_boolean() {
        let s = schema_of(vec![(
            "root",
            obj(vec![("toc", SchemaNode::Boolean)], false),
        )]);
        let v = map(vec![entry("toc", scalar(ScalarType::String))]);
        let errors = validate(&s, "root", &v);
        assert_eq!(errors.len(), 1);
        assert!(
            matches!(&errors[0].kind, ErrorKind::TypeMismatch { expected } if expected == "a boolean")
        );
        // A real boolean is accepted.
        let ok = map(vec![entry("toc", scalar(ScalarType::Bool))]);
        assert!(validate(&s, "root", &ok).is_empty());
    }

    #[test]
    fn allof_merges_object_branches() {
        // root = allOf[ {title}, ref(more) ]; more = {author}, both open.
        let s = schema_of(vec![
            (
                "root",
                SchemaNode::AllOf {
                    of: vec![
                        obj(vec![("title", SchemaNode::String)], false),
                        SchemaNode::Ref {
                            id: "more".to_string(),
                        },
                    ],
                },
            ),
            ("more", obj(vec![("author", SchemaNode::String)], false)),
        ]);
        // Both keys are known via the merge → no errors.
        let v = map(vec![
            entry("title", scalar(ScalarType::String)),
            entry("author", scalar(ScalarType::String)),
        ]);
        assert!(validate(&s, "root", &v).is_empty());
    }

    #[test]
    fn anyof_picks_object_branch_not_null() {
        // root = anyOf[ null, {format} ]  (like front-matter's shape)
        let s = schema_of(vec![(
            "root",
            SchemaNode::AnyOf {
                of: vec![
                    SchemaNode::Null,
                    obj(vec![("format", SchemaNode::String)], true),
                ],
            },
        )]);
        // Unknown key routes through the object branch, not "expected null".
        let v = map(vec![entry("forrmat", scalar(ScalarType::String))]);
        let errors = validate(&s, "root", &v);
        assert_eq!(errors.len(), 1);
        assert!(matches!(&errors[0].kind, ErrorKind::UnknownKey { .. }));
        // A valid doc passes via the object branch.
        let ok = map(vec![entry("format", scalar(ScalarType::String))]);
        assert!(validate(&s, "root", &ok).is_empty());
    }

    #[test]
    fn pattern_properties_route_value() {
        // root closed object with a patternProperty for html-ish keys → closed
        // html options object.
        let s = schema_of(vec![(
            "root",
            SchemaNode::Object {
                properties: BTreeMap::new(),
                closed: false,
                pattern: vec![PatternProp {
                    re: "^html$".to_string(),
                    schema: Box::new(obj(vec![("toc", SchemaNode::Boolean)], true)),
                }],
            },
        )]);
        // html.toc bad type is caught through the pattern route.
        let v = map(vec![entry(
            "html",
            map(vec![entry("toc", scalar(ScalarType::String))]),
        )]);
        let errors = validate(&s, "root", &v);
        assert_eq!(errors.len(), 1);
        assert!(matches!(&errors[0].kind, ErrorKind::TypeMismatch { .. }));
    }

    #[test]
    fn enum_membership() {
        let s = schema_of(vec![(
            "root",
            obj(
                vec![(
                    "engine",
                    SchemaNode::Enum {
                        values: vec!["knitr".into(), "jupyter".into()],
                    },
                )],
                false,
            ),
        )]);
        // Bad enum value flagged.
        let bad = map(vec![entry(
            "engine",
            scalar_lit(ScalarType::String, "knit"),
        )]);
        let errors = validate(&s, "root", &bad);
        assert_eq!(errors.len(), 1);
        assert!(matches!(
            &errors[0].kind,
            ErrorKind::InvalidEnum { allowed } if allowed == &vec!["knitr".to_string(), "jupyter".to_string()]
        ));
        // Valid enum value accepted.
        let ok = map(vec![entry(
            "engine",
            scalar_lit(ScalarType::String, "jupyter"),
        )]);
        assert!(validate(&s, "root", &ok).is_empty());
    }

    #[test]
    fn ref_cycle_terminates() {
        let s = schema_of(vec![(
            "root",
            SchemaNode::Ref {
                id: "root".to_string(),
            },
        )]);
        // Must not stack-overflow.
        let v = map(vec![entry("x", scalar(ScalarType::String))]);
        assert!(validate(&s, "root", &v).is_empty());
    }

    #[test]
    fn did_you_mean_honors_budget() {
        // Distance-2 candidate must not be suggested under a budget of 1.
        assert_eq!(did_you_mean("cran", ["brand"].into_iter(), 1), None);
        // Distance-1 candidate is still suggested.
        assert_eq!(
            did_you_mean("pdf", ["pdfa"].into_iter(), 1),
            Some("pdfa".to_string())
        );
    }
}
