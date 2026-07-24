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
//!   -m, --mapping <FILE>       RML mapping file(s) to apply [repeatable]
//!       --ottr <FILE>          stOTTR template/instance file(s) to expand [repeatable]
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
    OutputFormat, apply_ontologies, apply_ottr_templates, apply_rml_mappings, apply_rules,
    format_results, load_file, parse_rules, run_sparql_query,
};
use ingress::NetworkPolicy;
use sparql_endpoint::{AuthConfig, OidcConfig};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

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

    /// RML mapping file(s) to apply — maps CSV/JSON/XML sources to RDF (repeatable)
    #[arg(short = 'm', long = "mapping", value_name = "FILE")]
    mapping: Vec<PathBuf>,

    /// stOTTR template/instance file(s) to expand — templates and instances may be
    /// split across files or combined in one (repeatable)
    #[arg(long = "ottr", value_name = "FILE")]
    ottr: Vec<PathBuf>,

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

    /// Maximum request body size for RDF write routes (`/{name}/data`,
    /// `/rdf-graph-store`, `/rdf-graphs/*`, `/upload`, `/{name}/shacl`), in bytes
    #[arg(
        long = "max-rdf-upload-bytes",
        value_name = "BYTES",
        default_value_t = 64 * 1024 * 1024,
        env = "DAGALOG_MAX_RDF_UPLOAD_BYTES"
    )]
    max_rdf_upload_bytes: usize,

    // ── Tier 1: Static API key ───────────────────────────────────────────────
    /// Shared API key for Bearer token auth; omit to disable (Tier 1)
    #[arg(long = "api-key", value_name = "KEY", env = "DAGALOG_API_KEY")]
    api_key: Option<String>,

    /// Protect read endpoints (GET /sparql, etc.) with the API key too
    #[arg(long = "require-auth-for-reads", env = "DAGALOG_AUTH_READS")]
    require_auth_for_reads: bool,

    // ── Tier 2: Generic OIDC ─────────────────────────────────────────────────
    /// OIDC provider base URL, e.g. "https://login.microsoftonline.com/{tenant}/v2.0" (Tier 2)
    #[arg(long = "oidc-issuer", value_name = "URL", env = "DAGALOG_OIDC_ISSUER")]
    oidc_issuer: Option<String>,

    /// Expected `aud` claim (resource URI or client ID), e.g. "api://dagalog"
    #[arg(
        long = "oidc-audience",
        value_name = "STR",
        env = "DAGALOG_OIDC_AUDIENCE"
    )]
    oidc_audience: Option<String>,

    /// Explicit JWKS URI (skips OIDC discovery)
    #[arg(
        long = "oidc-jwks-uri",
        value_name = "URL",
        env = "DAGALOG_OIDC_JWKS_URI"
    )]
    oidc_jwks_uri: Option<String>,

    /// JWT claim path holding the roles array (default: "roles"; Keycloak: "realm_access.roles")
    #[arg(
        long = "oidc-roles-claim",
        value_name = "STR",
        default_value = "roles",
        env = "DAGALOG_OIDC_ROLES_CLAIM"
    )]
    oidc_roles_claim: String,

    /// Role that grants Read access (default: "dagalog.Read")
    #[arg(
        long = "oidc-read-role",
        value_name = "NAME",
        default_value = "dagalog.Read",
        env = "DAGALOG_OIDC_READ_ROLE"
    )]
    oidc_read_role: String,

    /// Role that grants Write access (default: "dagalog.Write")
    #[arg(
        long = "oidc-write-role",
        value_name = "NAME",
        default_value = "dagalog.Write",
        env = "DAGALOG_OIDC_WRITE_ROLE"
    )]
    oidc_write_role: String,

    /// Role that grants Admin access (default: "dagalog.Admin")
    #[arg(
        long = "oidc-admin-role",
        value_name = "NAME",
        default_value = "dagalog.Admin",
        env = "DAGALOG_OIDC_ADMIN_ROLE"
    )]
    oidc_admin_role: String,

    /// Browser application client ID for MSAL.js (Azure) or Google Identity Services
    #[arg(
        long = "oidc-browser-client-id",
        value_name = "ID",
        env = "DAGALOG_OIDC_BROWSER_CLIENT_ID"
    )]
    oidc_browser_client_id: Option<String>,

    // ── Persistence ──────────────────────────────────────────────────────────
    /// Directory for durable persistence (redb changelog); omit for in-memory mode
    #[arg(long = "data-dir", value_name = "PATH", env = "DAGALOG_DATA_DIR")]
    data_dir: Option<PathBuf>,

    /// Force in-memory mode even if DAGALOG_DATA_DIR is set
    #[arg(long = "no-persist", env = "DAGALOG_NO_PERSIST")]
    no_persist: bool,

    /// Remote network access policy: deny (default), ignore, allow, or allow:<prefixes>
    ///
    /// deny:               return an error for LOAD, @context URLs, SERVICE (safe default)
    /// ignore:             silently skip all remote fetch operations
    /// allow:              perform HTTP fetches (SSRF hardening active)
    /// allow:<p1>[,<p2>]:  only fetch URLs whose string starts with one of the listed prefixes
    ///
    /// Example: --network allow:https://example.org/,https://data.gov/
    #[arg(
        long = "network",
        value_name = "POLICY",
        default_value = "deny",
        env = "DAGALOG_NETWORK"
    )]
    network: String,
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

fn parse_network_policy(s: &str) -> Result<NetworkPolicy, String> {
    match s.to_ascii_lowercase().as_str() {
        "deny" => Ok(NetworkPolicy::Deny),
        "ignore" => Ok(NetworkPolicy::Ignore),
        "allow" => Ok(NetworkPolicy::Allow),
        other if other.starts_with("allow:") => {
            let prefixes = other["allow:".len()..]
                .split(',')
                .filter(|p| !p.is_empty())
                .map(|p| p.to_string())
                .collect();
            Ok(NetworkPolicy::AllowList(prefixes))
        }
        other => Err(format!(
            "unknown network policy '{other}'; expected one of: \
             deny, ignore, allow, allow:<prefix>[,<prefix>…]"
        )),
    }
}

fn run(cli: Cli) -> Result<(), String> {
    let format: OutputFormat = cli
        .format
        .parse()
        .map_err(|e: String| format!("--format: {}", e))?;

    let network_policy =
        parse_network_policy(&cli.network).map_err(|e| format!("--network: {}", e))?;

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

    if !cli.mapping.is_empty() {
        if cli.verbose {
            for p in &cli.mapping {
                eprintln!("applying RML mapping: {}", p.display());
            }
        }
        let triples_before = datastore.named_graphs.quad_count;
        apply_rml_mappings(&mut datastore, &cli.mapping)?;
        if cli.verbose {
            eprintln!(
                "Triples after RML mapping: {} (+{})",
                datastore.named_graphs.quad_count,
                datastore
                    .named_graphs
                    .quad_count
                    .saturating_sub(triples_before)
            );
        }
    }

    if !cli.ottr.is_empty() {
        if cli.verbose {
            for p in &cli.ottr {
                eprintln!("expanding OTTR templates: {}", p.display());
            }
        }
        let triples_before = datastore.named_graphs.quad_count;
        apply_ottr_templates(&mut datastore, &cli.ottr)?;
        if cli.verbose {
            eprintln!(
                "Triples after OTTR expansion: {} (+{})",
                datastore.named_graphs.quad_count,
                datastore
                    .named_graphs
                    .quad_count
                    .saturating_sub(triples_before)
            );
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

    // When serving, collect rules for IncrementalReasoner (initial materialisation
    // happens inside serve_on_listener).  For one-shot queries, apply rules eagerly.
    let serve_rules: Vec<_> = if !cli.rules.is_empty() {
        if cli.verbose {
            for p in &cli.rules {
                eprintln!("loading rules: {}", p.display());
            }
        }
        if cli.serve {
            // Defer materialisation to IncrementalReasoner::new inside the server.
            parse_rules(&mut datastore, &cli.rules)?
        } else {
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
            Vec::new() // rules already applied; no need to pass to Config
        }
    } else {
        Vec::new()
    };

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

        eprintln!("Webpage ready at http://localhost:{}", cli.port);

        let auth = build_auth_config(&cli)?;
        let data_dir = if cli.no_persist {
            None
        } else {
            cli.data_dir.clone()
        };
        let config = sparql_endpoint::Config {
            bind_addr,
            base_iri,
            read_only: cli.read_only,
            max_query_timeout_secs: cli.query_timeout,
            auth,
            data_dir,
            max_rdf_upload_bytes: cli.max_rdf_upload_bytes,
            initial_rules: serve_rules,
            network_policy,
            ..Default::default()
        };
        let store = Arc::new(RwLock::new(datastore));
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| format!("failed to create async runtime: {}", e))?;
        runtime
            .block_on(sparql_endpoint::serve(store, config))
            .map_err(|e| format!("server error: {}", e))?;
    } else if let Some(sparql_str) = &sparql {
        let result = run_sparql_query(&datastore, sparql_str)?;
        print!("{}", format_results(&result, &format));
    } else if cli.data.is_empty()
        && cli.mapping.is_empty()
        && cli.ottr.is_empty()
        && cli.ontology.is_empty()
        && cli.rules.is_empty()
    {
        eprintln!("No data files or query provided. Run with --help for usage.");
    } else {
        eprintln!(
            "Loaded {} triples. Use --query, --query-file, or --serve.",
            datastore.named_graphs.quad_count
        );
    }

    Ok(())
}

fn build_auth_config(cli: &Cli) -> Result<AuthConfig, String> {
    if cli.api_key.is_some() && cli.oidc_issuer.is_some() {
        return Err("--api-key and --oidc-issuer cannot both be set".to_string());
    }

    if let Some(key) = &cli.api_key {
        return Ok(AuthConfig::ApiKey {
            key: key.clone(),
            require_for_reads: cli.require_auth_for_reads,
        });
    }

    if let Some(issuer) = &cli.oidc_issuer {
        let audience = cli
            .oidc_audience
            .as_ref()
            .ok_or_else(|| "--oidc-audience is required when --oidc-issuer is set".to_string())?;
        return Ok(AuthConfig::Oidc(OidcConfig {
            issuer: issuer.clone(),
            jwks_uri: cli.oidc_jwks_uri.clone(),
            audience: audience.clone(),
            roles_claim: cli.oidc_roles_claim.clone(),
            read_role: cli.oidc_read_role.clone(),
            write_role: cli.oidc_write_role.clone(),
            admin_role: cli.oidc_admin_role.clone(),
            browser_client_id: cli.oidc_browser_client_id.clone(),
        }));
    }

    Ok(AuthConfig::None)
}
