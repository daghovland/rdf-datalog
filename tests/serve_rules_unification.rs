/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Regression test for [#319](https://github.com/daghovland/rdf-datalog/issues/319):
//! the `--serve` CLI path must unify ontology-derived (OWL2RL) rules and
//! directly-supplied `.datalog`-file rules into a single `Vec<Rule>` handed
//! to one `IncrementalReasoner`, rather than materialising the ontology
//! rules eagerly through a separate, untracked
//! `datalog::evaluate_rules` call.
//!
//! This test calls `dagalog::collect_serve_rules` directly — the exact
//! function `src/main.rs`'s `--serve` branch calls to build `serve_rules`
//! (see the `if cli.serve { ... }` block in `run()`). Before the fix, no
//! such function existed: the `--serve` path called `apply_ontologies`
//! (which evaluates ontology rules immediately and eagerly, discarding them
//! from `IncrementalReasoner`'s view) and only passed the `.datalog`-file
//! rules from `parse_rules` to the reasoner. Reverting the `collect_serve_rules`
//! implementation to instead call `apply_ontologies` and return only the
//! `.datalog` rules would make this test fail, because the OWL2RL-derived
//! rule would be missing from the returned `Vec<Rule>`.

use dagalog::collect_serve_rules;
use std::io::Write;

/// Both an ontology-derived rule (from an `.omn` TBox axiom) and a plain
/// `.datalog`-file rule must come back in the single merged `Vec<Rule>`,
/// proving neither source is silently dropped or evaluated-and-discarded
/// before the caller can hand them to a shared reasoner.
#[test]
fn collect_serve_rules_merges_ontology_and_datalog_rule_sources() {
    let mut ds = dag_rdf::Datastore::new(1024);

    // An .omn ontology whose TBox axiom (Dog subClassOf Animal) compiles to
    // at least one OWL2RL Datalog rule via owl2datalog.
    let mut ontology_file = tempfile::Builder::new()
        .suffix(".omn")
        .tempfile()
        .expect("create temp ontology file");
    writeln!(
        ontology_file,
        "Prefix: ex: <http://ex/>\n\
         Ontology: <http://ex/onto>\n\
         Class: ex:Animal\n\
         Class: ex:Dog\n\
         SubClassOf: ex:Animal\n"
    )
    .expect("write ontology fixture");
    let ontology_path = ontology_file.path().to_path_buf();

    // A plain .datalog file rule, unrelated to the ontology.
    let mut rules_file = tempfile::Builder::new()
        .suffix(".datalog")
        .tempfile()
        .expect("create temp rules file");
    writeln!(
        rules_file,
        "[?x, <http://ex/q>, ?y] :- [?x, <http://ex/p>, ?y] ."
    )
    .expect("write datalog fixture");
    let rules_path = rules_file.path().to_path_buf();

    let (rules, stats) = collect_serve_rules(&mut ds, &[ontology_path], &[rules_path])
        .expect("collect_serve_rules must succeed");

    assert!(
        stats.ontology_rule_count > 0,
        "the ontology's SubClassOf axiom must compile to at least one OWL2RL rule"
    );
    assert_eq!(
        rules.len(),
        stats.ontology_rule_count + 1,
        "the merged Vec<Rule> must contain BOTH the ontology-derived rule(s) AND the \
         .datalog-file rule — this is the crux of #319: before the fix, ontology rules \
         were evaluated eagerly by a separate untracked pass and never appeared in the \
         rules handed to IncrementalReasoner"
    );
}
