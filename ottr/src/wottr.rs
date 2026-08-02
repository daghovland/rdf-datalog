//! wOTTR: the RDF/Turtle serialisation of OTTR templates and instances.
//!
//! Spec: <https://spec.ottr.xyz/wOTTR/0.4.5/>. This module is a second front
//! end alongside [`crate::parser::parse_stottr`]: it reads `ottr:Template`/
//! `ottr:Instance`-shaped triples out of a [`Datastore`] and builds the same
//! [`StottrDocument`] that the stOTTR text parser builds, so
//! [`crate::expander::expand`]/[`crate::expand_documents`] need no changes.
//!
//! See `docs/plans/WOTTR_PLAN.md` for the full vocabulary-to-AST mapping.
//! Tracked by [#246](https://github.com/daghovland/rdf-datalog/issues/246).
//!
//! Scope limitations (permissive: unsupported constructs are skipped with a
//! `log::warn!` rather than erroring, matching the existing stOTTR stance):
//! - Only the two-element `(rdf:List T)` / `(ottr:NEList T)` parameter-type
//!   encoding is understood; multi-level composed types (`ottr:LUB`,
//!   `ottr:Bot`, chained wrappers like `(rdf:List ottr:NEList xsd:int)`) fall
//!   back to `OttrType::Iri`.
//! - `ottr:zipMax` has no `ast::Expander` variant yet (same gap already noted
//!   for stOTTR in `docs/plans/OTTR_PLAN.md`).
//! - Custom `ottr:BaseTemplate`s beyond the built-in `ottr:Triple` are not
//!   resolved (there is nothing to expand them into).
//! - `ottr:nonBlank` is read but not enforced.

use crate::OttrError;
use crate::ast::{Argument, Expander, Instance, Parameter, StottrDocument, TemplateDef, Term};
use crate::types::OttrType;
use dag_rdf::ingress::GraphElementId;
use dag_rdf::{Datastore, GraphElement, IriReference, RdfResource};
use ingress::{RDF, RDF_FIRST, RDF_NIL, RDF_REST, RDF_TYPE};
use std::collections::{HashMap, HashSet};

const OTTR_NS: &str = "http://ns.ottr.xyz/0.4/";

/// Cached `GraphElementId`s of every wOTTR/RDF vocabulary term this parser
/// cares about. `None` means the term is never used as a resource in this
/// particular datastore (which is fine — the corresponding construct simply
/// never matches).
struct Vocab {
    rdf_type: Option<GraphElementId>,
    rdf_list: Option<GraphElementId>,
    template: Option<GraphElementId>,
    parameters: Option<GraphElementId>,
    variable: Option<GraphElementId>,
    param_type: Option<GraphElementId>,
    modifier: Option<GraphElementId>,
    default: Option<GraphElementId>,
    pattern: Option<GraphElementId>,
    annotation: Option<GraphElementId>,
    of: Option<GraphElementId>,
    values: Option<GraphElementId>,
    arguments: Option<GraphElementId>,
    value: Option<GraphElementId>,
    none: Option<GraphElementId>,
    optional: Option<GraphElementId>,
    cross: Option<GraphElementId>,
    zip_min: Option<GraphElementId>,
    zip_max: Option<GraphElementId>,
    list_expand: Option<GraphElementId>,
    iri_type: Option<GraphElementId>,
    blank_node_type: Option<GraphElementId>,
    literal_type: Option<GraphElementId>,
    ne_list: Option<GraphElementId>,
}

fn lookup(ds: &Datastore, iri: &str) -> Option<GraphElementId> {
    let key = GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri.to_string())));
    ds.resources.resource_map.get(&key).copied()
}

impl Vocab {
    fn build(ds: &Datastore) -> Self {
        let ottr = |local: &str| lookup(ds, &format!("{OTTR_NS}{local}"));
        Vocab {
            rdf_type: lookup(ds, RDF_TYPE),
            rdf_list: lookup(ds, &format!("{RDF}List")),
            template: ottr("Template"),
            parameters: ottr("parameters"),
            variable: ottr("variable"),
            param_type: ottr("type"),
            modifier: ottr("modifier"),
            default: ottr("default"),
            pattern: ottr("pattern"),
            annotation: ottr("annotation"),
            of: ottr("of"),
            values: ottr("values"),
            arguments: ottr("arguments"),
            value: ottr("value"),
            none: ottr("none"),
            optional: ottr("optional"),
            cross: ottr("cross"),
            zip_min: ottr("zipMin"),
            zip_max: ottr("zipMax"),
            list_expand: ottr("listExpand"),
            iri_type: ottr("IRI"),
            blank_node_type: ottr("BlankNode"),
            literal_type: ottr("Literal"),
            ne_list: ottr("NEList"),
        }
    }
}

/// A stable, unique key for a blank node used as a parameter's `ottr:variable`.
/// Keyed off the blank node's own `GraphElementId`, which is unique per
/// parsed dataset, so it round-trips exactly between where a parameter
/// declares the variable and where pattern instances reference it.
fn variable_key(id: GraphElementId) -> String {
    format!("wottr_var_{id}")
}

/// Read `id`'s single object under `predicate`, if any (first match; wOTTR
/// properties that are meant to be single-valued, e.g. `ottr:of`, are only
/// ever asserted once per well-formed document).
fn single_object(
    ds: &Datastore,
    id: GraphElementId,
    predicate: GraphElementId,
) -> Option<GraphElementId> {
    ds.get_triples_with_subject_predicate(id, predicate)
        .next()
        .map(|t| t.obj)
}

fn all_objects(
    ds: &Datastore,
    id: GraphElementId,
    predicate: GraphElementId,
) -> Vec<GraphElementId> {
    ds.get_triples_with_subject_predicate(id, predicate)
        .map(|t| t.obj)
        .collect()
}

/// Returns true if `id` is the head of an RDF list, i.e. has an `rdf:first`.
fn is_list_head(ds: &Datastore, id: GraphElementId, rdf_first: GraphElementId) -> bool {
    ds.get_triples_with_subject_predicate(id, rdf_first)
        .next()
        .is_some()
}

/// Walk an RDF list (`rdf:first`/`rdf:rest` chain) starting at `head`,
/// returning its elements in order. Stops (without erroring) at the first
/// malformed link, since wOTTR documents are otherwise ordinary RDF and we'd
/// rather degrade gracefully than fail the whole parse over one bad list.
fn read_rdf_list(
    ds: &Datastore,
    head: GraphElementId,
    rdf_nil: Option<GraphElementId>,
) -> Vec<GraphElementId> {
    let rdf_first = match lookup(ds, RDF_FIRST) {
        Some(id) => id,
        None => return Vec::new(),
    };
    let rdf_rest = match lookup(ds, RDF_REST) {
        Some(id) => id,
        None => return Vec::new(),
    };
    let mut items = Vec::new();
    let mut node = head;
    loop {
        if Some(node) == rdf_nil {
            break;
        }
        let first = single_object(ds, node, rdf_first);
        let rest = single_object(ds, node, rdf_rest);
        match (first, rest) {
            (Some(f), Some(r)) => {
                items.push(f);
                node = r;
            }
            _ => break,
        }
    }
    items
}

fn term_from_id(ds: &Datastore, id: GraphElementId) -> Term {
    match ds.resources.get_graph_element(id) {
        GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => Term::Iri(iri.clone()),
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(n)) => {
            Term::BlankNode(format!("wottr_blank_{n}"))
        }
        GraphElement::GraphLiteral(lit) => Term::Literal(lit.clone()),
        GraphElement::TripleTerm(_) => {
            log::warn!(
                "wOTTR: RDF 1.2 triple terms are not supported as argument values; see #143"
            );
            Term::Iri(IriReference(format!(
                "urn:wottr:unsupported-triple-term:{id}"
            )))
        }
    }
}

/// Resolve a term appearing as an argument/parameter-default value: a
/// variable (if `id` is a known parameter-variable blank node), `ottr:none`,
/// a nested RDF list (recursively resolved), or a plain term.
fn resolve_value(
    ds: &Datastore,
    vocab: &Vocab,
    var_names: &HashMap<GraphElementId, String>,
    rdf_first: GraphElementId,
    id: GraphElementId,
) -> Argument {
    if Some(id) == vocab.none {
        return Argument::None;
    }
    if let Some(name) = var_names.get(&id) {
        return Argument::Term(Term::Variable(name.clone()));
    }
    let rdf_nil = lookup(ds, RDF_NIL);
    if Some(id) == rdf_nil || is_list_head(ds, id, rdf_first) {
        let items = read_rdf_list(ds, id, rdf_nil);
        return Argument::List(
            items
                .into_iter()
                .map(|item| resolve_value(ds, vocab, var_names, rdf_first, item))
                .collect(),
        );
    }
    Argument::Term(term_from_id(ds, id))
}

/// Map a parameter's `ottr:type` object to an `OttrType`. `type_id == None`
/// means no `ottr:type` was asserted, which defaults to `OttrType::Iri`
/// (matching `parser::parse_stottr`'s default).
fn resolve_param_type(ds: &Datastore, vocab: &Vocab, type_id: Option<GraphElementId>) -> OttrType {
    let Some(type_id) = type_id else {
        return OttrType::Iri;
    };
    if Some(type_id) == vocab.iri_type {
        return OttrType::Iri;
    }
    if Some(type_id) == vocab.blank_node_type {
        return OttrType::BlankNode;
    }
    if Some(type_id) == vocab.literal_type {
        return OttrType::Literal(None);
    }
    let rdf_first = lookup(ds, RDF_FIRST);
    if let Some(rdf_first) = rdf_first
        && is_list_head(ds, type_id, rdf_first)
    {
        let items = read_rdf_list(ds, type_id, lookup(ds, RDF_NIL));
        if items.len() == 2 {
            let wrapper = items[0];
            let inner = resolve_param_type(ds, vocab, Some(items[1]));
            if Some(wrapper) == vocab.rdf_list {
                return OttrType::List(Box::new(inner));
            }
            if Some(wrapper) == vocab.ne_list {
                return OttrType::NEList(Box::new(inner));
            }
        }
        log::warn!(
            "wOTTR: composed/multi-level parameter type at resource {type_id} is not supported, defaulting to ottr:IRI"
        );
        return OttrType::Iri;
    }
    // Any other atomic IRI (e.g. an xsd: datatype) names a literal datatype.
    match ds.resources.get_graph_element(type_id) {
        GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => OttrType::Literal(Some(iri.clone())),
        _ => {
            log::warn!(
                "wOTTR: unrecognised parameter type at resource {type_id}, defaulting to ottr:IRI"
            );
            OttrType::Iri
        }
    }
}

fn parse_parameter(
    ds: &Datastore,
    vocab: &Vocab,
    var_names: &mut HashMap<GraphElementId, String>,
    rdf_first: GraphElementId,
    param_node: GraphElementId,
) -> Parameter {
    let variable_id = match vocab
        .variable
        .and_then(|p| single_object(ds, param_node, p))
    {
        Some(id) => id,
        None => {
            log::warn!("wOTTR: parameter {param_node} has no ottr:variable, skipping");
            param_node
        }
    };
    let key = variable_key(variable_id);
    var_names.insert(variable_id, key.clone());

    let type_id = vocab
        .param_type
        .and_then(|p| single_object(ds, param_node, p));
    let ottr_type = resolve_param_type(ds, vocab, type_id);

    let modifiers: HashSet<GraphElementId> = vocab
        .modifier
        .map(|p| all_objects(ds, param_node, p).into_iter().collect())
        .unwrap_or_default();
    let optional = vocab.optional.is_some_and(|o| modifiers.contains(&o));

    let default = vocab
        .default
        .and_then(|p| single_object(ds, param_node, p))
        .map(|id| resolve_value(ds, vocab, var_names, rdf_first, id));

    Parameter {
        variable: key,
        ottr_type,
        optional,
        default,
    }
}

fn parse_instance_node(
    ds: &Datastore,
    vocab: &Vocab,
    var_names: &HashMap<GraphElementId, String>,
    rdf_first: GraphElementId,
    node: GraphElementId,
) -> Result<Instance, OttrError> {
    let of_id = vocab
        .of
        .and_then(|p| single_object(ds, node, p))
        .ok_or_else(|| OttrError::Parse(format!("wOTTR instance {node} has no ottr:of")))?;
    let template = match ds.resources.get_graph_element(of_id) {
        GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => iri.clone(),
        _ => {
            return Err(OttrError::Parse(format!(
                "wOTTR instance {node}'s ottr:of does not resolve to an IRI"
            )));
        }
    };

    let modifiers: HashSet<GraphElementId> = vocab
        .modifier
        .map(|p| all_objects(ds, node, p).into_iter().collect())
        .unwrap_or_default();
    let expander = if vocab.cross.is_some_and(|c| modifiers.contains(&c)) {
        Some(Expander::Cross)
    } else if vocab.zip_min.is_some_and(|z| modifiers.contains(&z)) {
        Some(Expander::ZipMin)
    } else if vocab.zip_max.is_some_and(|z| modifiers.contains(&z)) {
        log::warn!(
            "wOTTR: ottr:zipMax is not supported yet, treating instance {node} as having no expander"
        );
        None
    } else {
        None
    };

    let arguments = if let Some(values_head) = vocab.values.and_then(|p| single_object(ds, node, p))
    {
        read_rdf_list(ds, values_head, lookup(ds, RDF_NIL))
            .into_iter()
            .map(|id| resolve_value(ds, vocab, var_names, rdf_first, id))
            .collect()
    } else if let Some(args_head) = vocab.arguments.and_then(|p| single_object(ds, node, p)) {
        read_rdf_list(ds, args_head, lookup(ds, RDF_NIL))
            .into_iter()
            .map(|arg_node| {
                let value_id = vocab
                    .value
                    .and_then(|p| single_object(ds, arg_node, p))
                    .unwrap_or(arg_node);
                let resolved = resolve_value(ds, vocab, var_names, rdf_first, value_id);
                let arg_modifiers: HashSet<GraphElementId> = vocab
                    .modifier
                    .map(|p| all_objects(ds, arg_node, p).into_iter().collect())
                    .unwrap_or_default();
                let has_list_expand = vocab
                    .list_expand
                    .is_some_and(|le| arg_modifiers.contains(&le));
                match (has_list_expand, resolved) {
                    (true, Argument::Term(Term::Variable(v))) => Argument::ListExpand(v),
                    (_, other) => other,
                }
            })
            .collect()
    } else {
        Vec::new()
    };

    Ok(Instance {
        template,
        arguments,
        expander,
    })
}

/// Read templates and instances out of `datastore`, per the wOTTR vocabulary,
/// and build a [`StottrDocument`] equivalent to what
/// [`crate::parser::parse_stottr`] would build from the corresponding stOTTR
/// text.
pub fn parse_wottr(datastore: &Datastore) -> Result<StottrDocument, OttrError> {
    let vocab = Vocab::build(datastore);
    let rdf_first = match lookup(datastore, RDF_FIRST) {
        Some(id) => id,
        // No RDF lists in the whole dataset at all -> definitely no wOTTR
        // templates/instances (every one of them requires at least one list).
        None => {
            return Ok(StottrDocument {
                templates: Vec::new(),
                instances: Vec::new(),
            });
        }
    };

    let template_ids: Vec<GraphElementId> = match (vocab.template, vocab.rdf_type) {
        (Some(template_class), Some(rdf_type)) => datastore
            .get_triples_with_object_predicate(template_class, rdf_type)
            .map(|t| t.subject)
            .collect(),
        _ => Vec::new(),
    };

    // Pass 1: collect every parameter's variable blank node across all
    // templates before parsing any pattern body, so that a template's body
    // can always resolve its own parameters' variables regardless of
    // iteration order.
    let mut var_names: HashMap<GraphElementId, String> = HashMap::new();
    let mut template_params: HashMap<GraphElementId, Vec<Parameter>> = HashMap::new();
    for &template_id in &template_ids {
        let params_head = vocab
            .parameters
            .and_then(|p| single_object(datastore, template_id, p));
        let parameters = match params_head {
            Some(head) => read_rdf_list(datastore, head, lookup(datastore, RDF_NIL))
                .into_iter()
                .map(|param_node| {
                    parse_parameter(datastore, &vocab, &mut var_names, rdf_first, param_node)
                })
                .collect(),
            None => Vec::new(),
        };
        template_params.insert(template_id, parameters);
    }

    // Pass 2: parse each template's pattern body (a set, not a list, of
    // instances) now that every parameter variable is known.
    let mut templates = Vec::new();
    let mut visited_instance_nodes: HashSet<GraphElementId> = HashSet::new();
    for &template_id in &template_ids {
        let id = match datastore.resources.get_graph_element(template_id) {
            GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => iri.clone(),
            _ => {
                log::warn!("wOTTR: template {template_id} is not identified by an IRI, skipping");
                continue;
            }
        };
        let pattern_nodes = vocab
            .pattern
            .map(|p| all_objects(datastore, template_id, p))
            .unwrap_or_default();
        let mut body = Vec::new();
        for node in pattern_nodes {
            visited_instance_nodes.insert(node);
            body.push(parse_instance_node(
                datastore, &vocab, &var_names, rdf_first, node,
            )?);
        }
        templates.push(TemplateDef {
            id,
            parameters: template_params.remove(&template_id).unwrap_or_default(),
            body,
        });
    }

    // Annotation instances (`?signature ottr:annotation ?instance`) are owned
    // by whatever signature/template they annotate, exactly like pattern
    // instances — they must not leak into the top-level document instances.
    // Per the wOTTR SHACL grammar, `ottr:pattern` and `ottr:annotation` are
    // the complete set of "this instance belongs to something else" edges.
    // Annotations can be attached to any `ottr:Signature` (not just the
    // `ottr:Template`s collected above), so this scan is graph-wide.
    if let Some(annotation) = vocab.annotation {
        visited_instance_nodes.extend(
            datastore
                .get_triples_with_predicate(annotation)
                .map(|t| t.obj),
        );
    }

    // Pass 3: any remaining subject with an `ottr:of` that wasn't consumed as
    // part of some template's pattern or annotation is a top-level (document)
    // instance.
    let mut instances = Vec::new();
    if let Some(of) = vocab.of {
        let mut seen = HashSet::new();
        for t in datastore.get_triples_with_predicate(of) {
            if visited_instance_nodes.contains(&t.subject) || !seen.insert(t.subject) {
                continue;
            }
            instances.push(parse_instance_node(
                datastore, &vocab, &var_names, rdf_first, t.subject,
            )?);
        }
    }

    Ok(StottrDocument {
        templates,
        instances,
    })
}

/// Convenience wrapper: parse `text` as Turtle into a fresh [`Datastore`],
/// then run [`parse_wottr`] over it. Mainly used by tests and as the shape
/// later CLI/HTTP/Jupyter wiring (content-type dispatch) would use.
pub fn parse_wottr_str(text: &str) -> Result<StottrDocument, OttrError> {
    let mut datastore = Datastore::new(100);
    turtle::parse_turtle(&mut datastore, text.as_bytes())
        .map_err(|e| OttrError::Parse(format!("wOTTR turtle parse error: {e}")))?;
    parse_wottr(&datastore)
}
