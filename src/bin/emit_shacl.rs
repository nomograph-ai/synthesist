//! SHACL emitter for Synthesist schema.
//!
//! Reads the Rust schema constants from `src/schema/*.rs` (via the library
//! crate) and prints a valid Turtle SHACL document to stdout.
//!
//! Usage:
//!   cargo run --bin emit-shacl > ontology/synthesist.shacl.ttl
//!
//! The emitter is the authoritative source of truth for the SHACL file.
//! Do NOT edit `ontology/synthesist.shacl.ttl` by hand; run the Makefile
//! `shacl` target to regenerate it.

use nomograph_synthesist::schema::{campaign, discovery, outcome, phase, session, spec, task, tree};

/// Cardinality of a property in the shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Card {
    /// sh:minCount 1; sh:maxCount 1
    Required,
    /// sh:maxCount 1 (no minCount)
    Optional,
    /// sh:minCount 1 (no maxCount) -- required non-empty array
    RequiredMany,
    /// no maxCount, no minCount (multi-value literal bag)
    Many,
}

/// Value constraint for a property.
#[derive(Debug, Clone)]
enum Constraint {
    /// sh:datatype xsd:string
    XsdString,
    /// sh:in ( ... ) with enum values
    Enum(&'static [&'static str]),
    /// sh:nodeKind sh:Literal (used for multi-valued string arrays)
    NodeKindLiteral,
    /// sh:nodeKind sh:IRI (used for IRI-valued arrays like agree_snapshot)
    NodeKindIri,
}

/// One sh:property block.
#[derive(Debug, Clone)]
struct PropShape {
    path: &'static str,
    card: Card,
    constraint: Constraint,
}

impl PropShape {
    fn req_str(path: &'static str) -> Self {
        Self { path, card: Card::Required, constraint: Constraint::XsdString }
    }
    fn opt_str(path: &'static str) -> Self {
        Self { path, card: Card::Optional, constraint: Constraint::XsdString }
    }
    fn req_enum(path: &'static str, values: &'static [&'static str]) -> Self {
        Self { path, card: Card::Required, constraint: Constraint::Enum(values) }
    }
    fn opt_enum(path: &'static str, values: &'static [&'static str]) -> Self {
        Self { path, card: Card::Optional, constraint: Constraint::Enum(values) }
    }
    fn req_many_literal(path: &'static str) -> Self {
        Self { path, card: Card::RequiredMany, constraint: Constraint::NodeKindLiteral }
    }
    fn many_literal(path: &'static str) -> Self {
        Self { path, card: Card::Many, constraint: Constraint::NodeKindLiteral }
    }
    fn many_iri(path: &'static str) -> Self {
        Self { path, card: Card::Many, constraint: Constraint::NodeKindIri }
    }
}

/// One sh:NodeShape definition.
struct NodeShape {
    /// Title-cased class name, e.g. "Tree", "Spec".
    class: &'static str,
    /// Human label, e.g. "Tree shape".
    label: &'static str,
    /// Comment block that appears above the shape (single line).
    comment: &'static str,
    props: Vec<PropShape>,
}

fn shapes() -> Vec<NodeShape> {
    // The order here mirrors the hand-authored file.
    vec![
        NodeShape {
            class: "Tree",
            label: "Tree shape",
            comment: "Tree -- top-level project domain.",
            props: vec![
                PropShape::req_str("name"),
                PropShape::opt_str("description"),
            ],
        },
        NodeShape {
            class: "Spec",
            label: "Spec shape",
            comment: "Spec -- unit of work; goal + constraints + decisions.",
            props: vec![
                PropShape::req_str("tree"),
                PropShape::req_str("id"),
                PropShape::req_str("goal"),
                PropShape::req_enum("status", spec::STATUSES),
                PropShape::opt_str("constraints"),
                PropShape::opt_str("decisions"),
                PropShape::req_many_literal("topics"),
                PropShape::many_iri("agree_snapshot"),
            ],
        },
        NodeShape {
            class: "Task",
            label: "Task shape",
            comment: "Task -- atomic work item within a spec.",
            props: vec![
                PropShape::req_str("tree"),
                PropShape::req_str("spec"),
                PropShape::req_str("id"),
                PropShape::req_str("summary"),
                PropShape::req_enum("status", task::STATUSES),
                PropShape::opt_enum("gate", task::GATES),
                PropShape::opt_str("description"),
                PropShape::opt_str("owner"),
                PropShape::many_literal("depends_on"),
                PropShape::many_literal("files"),
            ],
        },
        NodeShape {
            class: "Discovery",
            label: "Discovery shape",
            comment: "Discovery -- append-only institutional memory per spec.",
            props: vec![
                PropShape::req_str("tree"),
                PropShape::req_str("spec"),
                PropShape::req_str("id"),
                PropShape::req_str("date"),
                PropShape::req_str("finding"),
                PropShape::opt_str("author"),
                PropShape::opt_str("impact"),
                PropShape::opt_str("action"),
            ],
        },
        NodeShape {
            class: "Session",
            label: "Session shape",
            comment: "Session -- isolated work copy with metadata.",
            props: vec![
                PropShape::req_str("id"),
                PropShape::opt_str("tree"),
                PropShape::opt_str("spec"),
                PropShape::opt_str("summary"),
            ],
        },
        NodeShape {
            class: "Phase",
            label: "Phase shape",
            comment: "Phase -- per-session workflow state.",
            props: vec![
                PropShape::req_str("session_id"),
                PropShape::req_enum("name", phase::NAMES),
            ],
        },
        NodeShape {
            class: "Campaign",
            label: "Campaign shape",
            comment: "Campaign -- cross-spec coordination.",
            props: vec![
                PropShape::req_str("tree"),
                PropShape::req_str("spec"),
                PropShape::req_enum("kind", campaign::KINDS),
                PropShape::opt_str("summary"),
                PropShape::opt_str("title"),
                PropShape::many_literal("blocked_by"),
            ],
        },
        NodeShape {
            class: "Outcome",
            label: "Outcome shape",
            comment: "Outcome -- what happened to a spec (distinct from Spec status).",
            props: vec![
                PropShape::req_str("tree"),
                PropShape::req_str("spec"),
                PropShape::req_enum("status", outcome::STATUSES),
                PropShape::opt_str("note"),
                PropShape::opt_str("linked_spec"),
                PropShape::opt_str("date"),
            ],
        },
    ]
}

fn emit_prop(prop: &PropShape) -> String {
    let mut lines = Vec::new();
    lines.push(format!("    sh:property ["));
    lines.push(format!("        sh:path synthesist:{} ;", prop.path));

    match prop.card {
        Card::Required => {
            lines.push("        sh:minCount 1 ;".to_string());
            lines.push("        sh:maxCount 1 ;".to_string());
        }
        Card::Optional => {
            lines.push("        sh:maxCount 1 ;".to_string());
        }
        Card::RequiredMany => {
            lines.push("        sh:minCount 1 ;".to_string());
        }
        Card::Many => {
            // no minCount / maxCount lines
        }
    }

    match &prop.constraint {
        Constraint::XsdString => {
            lines.push("        sh:datatype xsd:string ;".to_string());
        }
        Constraint::Enum(values) => {
            let items: Vec<String> = values.iter().map(|v| format!("\"{}\"", v)).collect();
            lines.push(format!("        sh:in ( {} ) ;", items.join(" ")));
        }
        Constraint::NodeKindLiteral => {
            lines.push("        sh:nodeKind sh:Literal ;".to_string());
        }
        Constraint::NodeKindIri => {
            lines.push("        sh:nodeKind sh:IRI ;".to_string());
        }
    }

    lines.push("    ] ;".to_string());
    lines.join("\n")
}

fn emit_shape(shape: &NodeShape) -> String {
    use nomograph_synthesist::wire_format::{shape_iri, type_iri};
    let mut out = String::new();

    out.push_str(&format!("#\n# {}\n#\n\n", shape.comment));

    // Use the wire_format builders so the SHACL emitter, dual-write,
    // and migration all agree on shape and type IRI conventions.
    // `shape.class` is the v2-era TitleCase form (e.g. "Tree"); the
    // wire_format helpers take a snake-case input and TitleCase it.
    // Pass `shape.class` as-is because TitleCase is idempotent under
    // the helper (single-word TitleCase input survives unchanged).
    out.push_str(&format!("{} a sh:NodeShape ;\n", shape_iri(shape.class)));
    out.push_str(&format!("    sh:targetClass {} ;\n", type_iri(shape.class)));
    out.push_str(&format!(
        "    rdfs:label \"{}\"@en ;\n",
        shape.label
    ));

    for (i, prop) in shape.props.iter().enumerate() {
        let prop_str = emit_prop(prop);
        if i == shape.props.len() - 1 {
            // Last property: replace trailing " ;" with " ."
            let trimmed = prop_str.strip_suffix(" ;").unwrap_or(&prop_str);
            out.push_str(&format!("{} .\n", trimmed));
        } else {
            out.push_str(&format!("{}\n", prop_str));
        }
    }

    out
}

fn emit() -> String {
    // Suppress unused-import warnings; these are used via the shapes() fn.
    let _ = discovery::TYPE_NAME;
    let _ = session::TYPE_NAME;
    let _ = tree::TYPE_NAME;

    let mut out = String::new();

    out.push_str("@prefix synthesist: <https://nomograph.org/synthesist/> .\n");
    out.push_str("@prefix nomograph: <https://nomograph.org/v3/> .\n");
    out.push_str("@prefix prov: <http://www.w3.org/ns/prov#> .\n");
    out.push_str("@prefix sh: <http://www.w3.org/ns/shacl#> .\n");
    out.push_str("@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .\n");
    out.push_str("@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n");
    out.push_str("@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n");
    out.push('\n');

    out.push_str("#\n");
    out.push_str("# Synthesist (synthesist:) module SHACL shapes.\n");
    out.push_str("#\n");
    out.push_str("# Documentation artifact. NOT a runtime gate. Synthesist's imperative\n");
    out.push_str("# validators in src/schema/*.rs remain authoritative; SHACL ships\n");
    out.push_str("# alongside the binary as a declarative description for LLM and\n");
    out.push_str("# external-tool consumption.\n");
    out.push_str("#\n");
    out.push_str("# Source of truth for each enum (sh:in lists): the Rust constants\n");
    out.push_str("# in src/schema/<type>.rs. This file is generated by `emit-shacl`;\n");
    out.push_str("# do NOT edit by hand. Run `make shacl` to regenerate.\n");
    out.push_str("#\n");
    out.push_str("# Every shape extends the universal envelope by including the four\n");
    out.push_str("# substrate-universal predicates: @id (sh:targetClass binds this),\n");
    out.push_str("# rdf:type, prov:generatedAtTime, prov:wasAttributedTo. The base\n");
    out.push_str("# shape in nomograph-claim's ontology/base.shacl.ttl covers those\n");
    out.push_str("# constraints; each shape here adds per-type constraints.\n");
    out.push_str("#\n");
    out.push('\n');

    for (i, shape) in shapes().iter().enumerate() {
        out.push_str(&emit_shape(shape));
        if i < shapes().len() - 1 {
            out.push('\n');
        }
    }

    out
}

fn main() {
    print!("{}", emit());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emit_produces_non_empty_output() {
        use nomograph_synthesist::wire_format::shape_iri;
        let out = emit();
        assert!(!out.is_empty());
        assert!(out.contains(&shape_iri("tree")));
        assert!(out.contains(&shape_iri("spec")));
    }

    #[test]
    fn spec_statuses_present() {
        let out = emit();
        assert!(out.contains("\"draft\" \"active\" \"done\" \"superseded\""));
    }

    #[test]
    fn task_statuses_present() {
        let out = emit();
        assert!(out.contains(
            "\"pending\" \"in_progress\" \"done\" \"blocked\" \"waiting\" \"cancelled\""
        ));
    }
}
