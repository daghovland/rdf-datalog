/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! dagalog — RDF triplestore with OWL-RL reasoning, Datalog rules, and SPARQL.
//!
//! # Usage
//!
//! ```text
//! dagalog [OPTIONS]
//!
//! Options:
//!   -d, --data <FILE>          Turtle/TriG data file(s) to load [repeatable]
//!   -o, --ontology <FILE>      OWL ontology file(s) → OWL-RL reasoning [repeatable]
//!   -r, --rules <FILE>         Datalog rules file(s) [repeatable]
//!   -Q, --query <SPARQL|FILE>  Inline SPARQL SELECT or path to .sparql file
//!   -q, --query-file <FILE>    SPARQL SELECT query file (alternative to --query)
//!   -f, --format <FORMAT>      Output: table (default), csv, json
//!   -v, --verbose              Print pipeline stats to stderr
//!       --serve                Start SPARQL HTTP endpoint
//!       --port <PORT>          Port to listen on [default: 3030]
//!       --base-iri <IRI>       Base IRI for Service Description
//!   -h, --help                 Print help
//! ```

use clap::Parser;
use dag_rdf::Datastore;
use dagalog::{
    OutputFormat, apply_ontologies, apply_rules, format_results, load_file, run_sparql_query,
};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(
    name = "dagalog",
    about = "RDF triplestore with OWL-RL reasoning, Datalog rules, and SPARQL",
    long_about = "Load Turtle/TriG data, optionally apply OWL-RL reasoning and custom Datalog\n\
                  rules, then answer SPARQL SELECT queries or serve a SPARQL HTTP endpoint."
)]
struct Cli {
    /// Turtle or TriG data file(s) to load (repeatable)
    #[arg(short = 'd', long = "data", value_name = "FILE")]
    data: Vec<PathBuf>,

    /// OWL ontology file(s) — loads and applies OWL-RL reasoning (repeatable)
    #[arg(short = 'o', long = "ontology", value_name = "FILE")]
    ontology: Vec<PathBuf>,

    /// Datalog rules file(s) to load and apply (repeatable)
    #[arg(short = 'r', long = "rules", value_name = "FILE")]
    rules: Vec<PathBuf>,

    /// Inline SPARQL SELECT query or path to a .sparql file
    #[arg(short = 'Q', long = "query", value_name = "SPARQL|FILE")]
    query: Option<String>,

    /// SPARQL SELECT query file (alternative to --query)
    #[arg(short = 'q', long = "query-file", value_name = "FILE")]
    query_file: Option<PathBuf>,

    /// Output format: table (default), csv, json
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

    /// Start a SPARQL 1.1 HTTP endpoint instead of running a one-shot query
    #[arg(long = "serve")]
    serve: bool,

    /// Port to listen on when --serve is set
    #[arg(
        long = "port",
        value_name = "PORT",
        default_value = "3030",
        env = "DAGALOG_PORT"
    )]
    port: u16,

    /// Base IRI for the SPARQL Service Description
    #[arg(long = "base-iri", value_name = "IRI", env = "DAGALOG_BASE_IRI")]
    base_iri: Option<String>,

    /// Disable all mutating endpoints (PUT, POST, DELETE, SPARQL Update)
    #[arg(long = "read-only", env = "DAGALOG_READ_ONLY")]
    read_only: bool,

    /// Maximum query execution time in seconds
    #[arg(
        long = "query-timeout",
        value_name = "SECS",
        default_value = "30",
        env = "DAGALOG_QUERY_TIMEOUT"
    )]
    query_timeout: u64,
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), String> {
    let format: OutputFormat = cli
        .format
        .parse()
        .map_err(|e: String| format!("--format: {}", e))?;

    // Resolve SPARQL query string: --query-file wins; --query auto-detects file paths.
    let sparql = match (&cli.query_file, &cli.query) {
        (Some(_), Some(_)) => return Err("--query-file and --query cannot both be set".to_string()),
        (Some(path), None) => Some(
            std::fs::read_to_string(path)
                .map_err(|e| format!("cannot read query file {}: {}", path.display(), e))?,
        ),
        (None, Some(q)) => {
            let p = std::path::Path::new(q.as_str());
            if p.is_file() {
                Some(
                    std::fs::read_to_string(p)
                        .map_err(|e| format!("cannot read {}: {}", p.display(), e))?,
                )
            } else {
                Some(q.clone())
            }
        }
        (None, None) => None,
    };

    // ── Build the datastore ──────────────────────────────────────────────────
    let mut datastore = Datastore::new(1_000_000);

    for path in &cli.data {
        if cli.verbose {
            eprintln!("loading data: {}", path.display());
        }
        load_file(&mut datastore, path)?;
        if cli.verbose {
            eprintln!("  triples: {}", datastore.named_graphs.quad_count);
        }
    }

    if !cli.ontology.is_empty() {
        if cli.verbose {
            for p in &cli.ontology {
                eprintln!("loading ontology: {}", p.display());
            }
        }
        let stats = apply_ontologies(&mut datastore, &cli.ontology)?;
        if cli.verbose {
            eprintln!("OWL axioms extracted: {}", stats.axiom_count);
            eprintln!("Datalog rules generated: {}", stats.rule_count);
            eprintln!(
                "Triples after OWL reasoning: {} (+{})",
                stats.triples_after,
                stats.triples_after.saturating_sub(stats.triples_before)
            );
        }
    }

    if !cli.rules.is_empty() {
        if cli.verbose {
            for p in &cli.rules {
                eprintln!("loading rules: {}", p.display());
            }
        }
        let triples_before = datastore.named_graphs.quad_count;
        let rule_count = apply_rules(&mut datastore, &cli.rules)?;
        if cli.verbose {
            eprintln!("Datalog rules applied: {}", rule_count);
            eprintln!(
                "Triples after Datalog materialisation: {} (+{})",
                datastore.named_graphs.quad_count,
                datastore
                    .named_graphs
                    .quad_count
                    .saturating_sub(triples_before)
            );
        }
    }

    if cli.verbose {
        eprintln!("total triples: {}", datastore.named_graphs.quad_count);
    }

    // ── Serve or query ───────────────────────────────────────────────────────
    if cli.serve {
        let base_iri = cli
            .base_iri
            .clone()
            .unwrap_or_else(|| format!("http://localhost:{}", cli.port));
        let bind_addr: std::net::SocketAddr = format!("0.0.0.0:{}", cli.port)
            .parse()
            .map_err(|e| format!("invalid port {}: {}", cli.port, e))?;

        eprintln!(
            "SPARQL endpoint ready at http://localhost:{}/sparql",
            cli.port
        );

        let config = sparql_endpoint::Config {
            bind_addr,
            base_iri,
            read_only: cli.read_only,
            max_query_timeout_secs: cli.query_timeout,
        };
        let store = Arc::new(tokio::sync::RwLock::new(datastore));
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| format!("failed to create async runtime: {}", e))?;
        runtime
            .block_on(sparql_endpoint::serve(store, config))
            .map_err(|e| format!("server error: {}", e))?;
    } else if let Some(sparql_str) = &sparql {
        let result = run_sparql_query(&datastore, sparql_str)?;
        print!("{}", format_results(&result, &format));
    } else if cli.data.is_empty() && cli.ontology.is_empty() && cli.rules.is_empty() {
        eprintln!("No data files or query provided. Run with --help for usage.");
    } else {
        eprintln!(
            "Loaded {} triples. Use --query, --query-file, or --serve.",
            datastore.named_graphs.quad_count
        );
    }

    Ok(())
}
