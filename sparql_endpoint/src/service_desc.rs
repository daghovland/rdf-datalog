/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! SPARQL 1.1 Service Description (`GET /sparql` with no query param).
//!
//! Spec: <https://www.w3.org/TR/sparql11-service-description/>

/// Generate the Service Description as a Turtle document.
pub fn service_description_turtle(base_iri: &str) -> String {
    format!(
        r#"@prefix sd: <http://www.w3.org/ns/sparql-service-description#> .
@prefix void: <http://rdfs.org/ns/void#> .
@prefix formats: <http://www.w3.org/ns/formats/> .

<{base_iri}/sparql> a sd:Service ;
    sd:endpoint <{base_iri}/sparql> ;
    sd:supportedLanguage sd:SPARQL11Query ;
    sd:resultFormat formats:SPARQL_Results_JSON,
                    formats:SPARQL_Results_XML,
                    formats:N-Triples,
                    formats:Turtle ;
    sd:feature sd:BasicFederatedQuery ;
    sd:defaultDataset [ a sd:Dataset ;
        sd:defaultGraph [ a sd:Graph ]
    ] .
"#,
        base_iri = base_iri
    )
}
