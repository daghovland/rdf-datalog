use crate::ast::{Argument, Expander, Instance, TemplateDef, Term};
use crate::base_templates::OTTR_TRIPLE_IRI;
use crate::error::OttrError;
use dag_rdf::Datastore;
use dag_rdf::ingress::{GraphElementId, Triple};
use ingress::IriReference;
use std::collections::{HashMap, HashSet};

/// Expand a list of top-level instance calls into quads in `datastore`,
/// using `templates` to resolve user-defined templates.
pub fn expand(
    templates: &HashMap<IriReference, TemplateDef>,
    instances: &[Instance],
    datastore: &mut Datastore,
) -> Result<(), OttrError> {
    let bindings = HashMap::new();
    let mut call_stack = HashSet::new();
    for instance in instances {
        expand_instance(instance, &bindings, templates, datastore, &mut call_stack)?;
    }
    Ok(())
}

fn expand_instance(
    instance: &Instance,
    bindings: &HashMap<String, Argument>,
    templates: &HashMap<IriReference, TemplateDef>,
    datastore: &mut Datastore,
    call_stack: &mut HashSet<IriReference>,
) -> Result<(), OttrError> {
    let substituted: Vec<Argument> = instance
        .arguments
        .iter()
        .map(|arg| substitute_argument(arg, bindings))
        .collect();

    if let Some(expander) = &instance.expander {
        return match expander {
            Expander::Cross => expand_cross(
                instance,
                substituted,
                bindings,
                templates,
                datastore,
                call_stack,
            ),
            Expander::ZipMin => expand_zip_min(
                instance,
                substituted,
                bindings,
                templates,
                datastore,
                call_stack,
            ),
        };
    }

    if instance.template.0 == OTTR_TRIPLE_IRI {
        // `none` in any argument position drops this triple silently.
        if substituted.iter().any(|arg| matches!(arg, Argument::None)) {
            return Ok(());
        }
        let [subject, predicate, object] = substituted.as_slice() else {
            return Err(OttrError::ArityMismatch {
                template: OTTR_TRIPLE_IRI.to_string(),
                got: substituted.len(),
                expected: 3,
            });
        };
        let mut blank_nodes = HashMap::new();
        let subject = resolve_argument(subject, datastore, &mut blank_nodes)?;
        let predicate = resolve_argument(predicate, datastore, &mut blank_nodes)?;
        let object = resolve_argument(object, datastore, &mut blank_nodes)?;
        datastore.add_triple(Triple {
            subject,
            predicate,
            obj: object,
        });
        return Ok(());
    }

    if call_stack.contains(&instance.template) {
        return Err(OttrError::RecursiveTemplate(instance.template.0.clone()));
    }

    let template = templates
        .get(&instance.template)
        .ok_or_else(|| OttrError::UnknownTemplate(instance.template.0.clone()))?;
    if template.parameters.len() != substituted.len() {
        return Err(OttrError::ArityMismatch {
            template: instance.template.0.clone(),
            got: substituted.len(),
            expected: template.parameters.len(),
        });
    }

    let new_bindings: HashMap<String, Argument> = template
        .parameters
        .iter()
        .map(|param| param.variable.clone())
        .zip(substituted)
        .collect();
    call_stack.insert(instance.template.clone());
    for body_instance in &template.body {
        expand_instance(
            body_instance,
            &new_bindings,
            templates,
            datastore,
            call_stack,
        )?;
    }
    call_stack.remove(&instance.template);
    Ok(())
}

/// Replace bound variables in `arg` with their value from `bindings`.
/// `ListExpand(var)` is also resolved to the list the variable is bound to.
fn substitute_argument(arg: &Argument, bindings: &HashMap<String, Argument>) -> Argument {
    match arg {
        Argument::Term(Term::Variable(name)) => {
            bindings.get(name).cloned().unwrap_or(Argument::None)
        }
        Argument::ListExpand(name) => bindings.get(name).cloned().unwrap_or(Argument::None),
        Argument::List(items) => Argument::List(
            items
                .iter()
                .map(|item| substitute_argument(item, bindings))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Expand a `cross` instance: cartesian product over list-typed arguments;
/// scalar arguments are broadcast unchanged across all combinations.
fn expand_cross(
    instance: &Instance,
    substituted: Vec<Argument>,
    bindings: &HashMap<String, Argument>,
    templates: &HashMap<IriReference, TemplateDef>,
    datastore: &mut Datastore,
    call_stack: &mut HashSet<IriReference>,
) -> Result<(), OttrError> {
    let list_positions: Vec<usize> = substituted
        .iter()
        .enumerate()
        .filter_map(|(i, a)| matches!(a, Argument::List(_)).then_some(i))
        .collect();
    let list_vecs: Vec<Vec<Argument>> = list_positions
        .iter()
        .map(|&i| match &substituted[i] {
            Argument::List(items) => items.clone(),
            _ => unreachable!(),
        })
        .collect();
    for combo in cartesian_product(&list_vecs) {
        let mut args = substituted.clone();
        for (&pos, val) in list_positions.iter().zip(combo) {
            args[pos] = val;
        }
        let flat = Instance {
            template: instance.template.clone(),
            arguments: args,
            expander: None,
        };
        expand_instance(&flat, bindings, templates, datastore, call_stack)?;
    }
    Ok(())
}

/// Expand a `zipMin` instance: pair list arguments by index, truncate to the
/// shortest list; scalar arguments are broadcast unchanged.
fn expand_zip_min(
    instance: &Instance,
    substituted: Vec<Argument>,
    bindings: &HashMap<String, Argument>,
    templates: &HashMap<IriReference, TemplateDef>,
    datastore: &mut Datastore,
    call_stack: &mut HashSet<IriReference>,
) -> Result<(), OttrError> {
    let list_positions: Vec<usize> = substituted
        .iter()
        .enumerate()
        .filter_map(|(i, a)| matches!(a, Argument::List(_)).then_some(i))
        .collect();
    let min_len = list_positions
        .iter()
        .map(|&i| match &substituted[i] {
            Argument::List(items) => items.len(),
            _ => unreachable!(),
        })
        .min()
        .unwrap_or(0);
    for idx in 0..min_len {
        let mut args = substituted.clone();
        for &pos in &list_positions {
            args[pos] = match &substituted[pos] {
                Argument::List(items) => items[idx].clone(),
                _ => unreachable!(),
            };
        }
        let flat = Instance {
            template: instance.template.clone(),
            arguments: args,
            expander: None,
        };
        expand_instance(&flat, bindings, templates, datastore, call_stack)?;
    }
    Ok(())
}

/// Compute the cartesian product of a slice of `Vec<T>`.
fn cartesian_product<T: Clone>(lists: &[Vec<T>]) -> Vec<Vec<T>> {
    lists.iter().fold(vec![vec![]], |acc, list| {
        acc.iter()
            .flat_map(|prefix| {
                list.iter().map(move |item| {
                    let mut combo = prefix.clone();
                    combo.push(item.clone());
                    combo
                })
            })
            .collect()
    })
}

fn resolve_argument(
    arg: &Argument,
    datastore: &mut Datastore,
    blank_nodes: &mut HashMap<String, GraphElementId>,
) -> Result<GraphElementId, OttrError> {
    match arg {
        Argument::Term(term) => resolve_term(term, datastore, blank_nodes),
        Argument::None => unreachable!("none arguments are filtered out before resolution"),
        Argument::List(_) | Argument::ListExpand(_) => Err(OttrError::Parse(
            "list arguments are not yet supported in ottr:Triple position".to_string(),
        )),
    }
}

fn resolve_term(
    term: &Term,
    datastore: &mut Datastore,
    blank_nodes: &mut HashMap<String, GraphElementId>,
) -> Result<GraphElementId, OttrError> {
    match term {
        Term::Iri(iri) => Ok(datastore.add_node_resource(dag_rdf::RdfResource::Iri(iri.clone()))),
        Term::Literal(literal) => Ok(datastore.add_literal_resource(literal.clone())),
        Term::BlankNode(label) => {
            if let Some(id) = blank_nodes.get(label) {
                Ok(*id)
            } else {
                let id = datastore.new_anonymous_blank_node();
                blank_nodes.insert(label.clone(), id);
                Ok(id)
            }
        }
        Term::Variable(name) => Err(OttrError::UnboundVariable(name.clone())),
    }
}
