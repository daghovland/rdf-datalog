/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! dagalog — RDF triplestore with OWL-RL reasoning and SPARQL query answering.
//!
//! # Usage
//!
//! ```text
//! dagalog [OPTIONS]
//!
//! Options:
//!   -d, --data <FILE>         Turtle/TriG data file(s) to load [repeatable]
//!   -o, --ontology <FILE>     OWL ontology file(s) to apply (triggers OWL-RL reasoning) [repeatable]
//!   -r, --rules <FILE>        Datalog rules file(s) [not yet supported] [repeatable]
//!   -q, --query-file <FILE>   SPARQL SELECT query file
//!   -Q, --query <SPARQL>      Inline SPARQL SELECT query string
//!   -f, --format <FORMAT>     Output format: table, csv, json [default: table]
//!   -v, --verbose             Print pipeline statistics to stderr
//!   -h, --help                Print help
//! ```

use clap::Parser;
use dag_rdf::Datastore;
use dagalog::{OutputFormat, apply_ontologies, format_results, load_file, run_sparql_query};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "dagalog",
    about = "RDF triplestore with OWL-RL reasoning and SPARQL query answering",
    long_about = "Load Turtle/TriG data, optionally apply OWL-RL reasoning from ontology files,\n\
                  and answer SPARQL SELECT queries."
)]
struct Cli {
    /// Turtle or TriG data file(s) to load (may be given multiple times)
    #[arg(short = 'd', long = "data", value_name = "FILE")]
    data: Vec<PathBuf>,

    /// OWL ontology Turtle file(s) to load and apply OWL-RL reasoning (may be given multiple times)
    #[arg(short = 'o', long = "ontology", value_name = "FILE")]
    ontology: Vec<PathBuf>,

    /// Datalog rules file(s) — NOT YET SUPPORTED (may be given multiple times)
    #[arg(short = 'r', long = "rules", value_name = "FILE")]
    rules: Vec<PathBuf>,

    /// SPARQL SELECT query file
    #[arg(short = 'q', long = "query-file", value_name = "FILE")]
    query_file: Option<PathBuf>,

    /// Inline SPARQL SELECT query string
    #[arg(short = 'Q', long = "query", value_name = "SPARQL")]
    query: Option<String>,

    /// Output format: table, csv, json [default: table]
    #[arg(
        short = 'f',
        long = "format",
        value_name = "FORMAT",
        default_value = "table"
    )]
    format: String,

    /// Print pipeline statistics to stderr
    #[arg(short = 'v', long = "verbose")]
    verbose: bool,
}

fn main() {
    let cli = Cli::parse();

    if let Err(e) = run(cli) {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), String> {
    // Reject --rules early — datalog parser is not yet implemented
    if !cli.rules.is_empty() {
        return Err(
            "--rules is not yet supported: the Datalog parser has not been implemented.\n\
             To implement it, translate DagSemTools.Datalog.Parser from F# similarly to\n\
             how the SPARQL and Turtle parsers were translated."
                .to_string(),
        );
    }

    // Parse output format
    let format: OutputFormat = cli
        .format
        .parse()
        .map_err(|e: String| format!("--format: {}", e))?;

    // Resolve SPARQL query (either from file or inline).
    // --query accepts both an inline SPARQL string and a file path; if the value
    // refers to an existing file it is read as a file, otherwise used as-is.
    let sparql = match (&cli.query_file, &cli.query) {
        (Some(_), Some(_)) => {
            return Err("--query-file and --query cannot be used together".to_string());
        }
        (Some(path), None) => Some(
            std::fs::read_to_string(path)
                .map_err(|e| format!("cannot read query file {}: {}", path.display(), e))?,
        ),
        (None, Some(q)) => {
            let path = std::path::Path::new(q);
            if path.is_file() {
                Some(
                    std::fs::read_to_string(path)
                        .map_err(|e| format!("cannot read query file {}: {}", path.display(), e))?,
                )
            } else {
                Some(q.clone())
            }
        }
        (None, None) => None,
    };

    // Build datastore
    let mut datastore = Datastore::new(1_000_000);

    // Load data files
    for path in &cli.data {
        if cli.verbose {
            eprintln!("loading data: {}", path.display());
        }
        load_file(&mut datastore, path)?;
        if cli.verbose {
            eprintln!("  triples: {}", datastore.named_graphs.quad_count);
        }
    }

    // Load ontologies and apply OWL-RL reasoning
    if !cli.ontology.is_empty() {
        if cli.verbose {
            for path in &cli.ontology {
                eprintln!("loading ontology: {}", path.display());
            }
        }
        let stats = apply_ontologies(&mut datastore, &cli.ontology)?;
        if cli.verbose {
            eprintln!("OWL axioms extracted: {}", stats.axiom_count);
            eprintln!("Datalog rules generated: {}", stats.rule_count);
            eprintln!(
                "Triples after reasoning: {} (inferred: {})",
                stats.triples_after,
                stats.triples_after.saturating_sub(stats.triples_before)
            );
        }
    }

    // Print triple count summary if verbose and no query
    if cli.verbose {
        eprintln!("total triples: {}", datastore.named_graphs.quad_count);
    }

    // Execute SPARQL query if given
    if let Some(sparql_str) = &sparql {
        let result = run_sparql_query(&datastore, sparql_str)?;
        print!("{}", format_results(&result, &format));
    } else if cli.data.is_empty() && cli.ontology.is_empty() {
        // No data, no query: print help hint
        eprintln!("No data files or query provided. Run with --help for usage.");
    } else {
        eprintln!(
            "Loaded {} triples. Use --query or --query-file to run a SPARQL query.",
            datastore.named_graphs.quad_count
        );
    }

    Ok(())
}
