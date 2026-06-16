# Third-Party Notices

This repository includes third-party data files used as test fixtures. They are
listed here with their sources, copyright holders, and licenses.

---

## W3C Test Suites

The following directories contain official W3C RDF/SPARQL test suite files,
distributed under the **W3C Test Suite License** and the **W3C 3-clause BSD
License**. Each directory contains a `LICENSE` file with the full license text.

| Directory | Test suite home |
|---|---|
| `tests/testdata/w3c_turtle/` | https://www.w3.org/2013/TurtleTests/ |
| `tests/testdata/w3c_trig/` | https://www.w3.org/2013/TrigTests/ |
| `tests/testdata/w3c_ntriples/` | https://www.w3.org/2013/N-TriplesTests/ |
| `tests/testdata/w3c_nquads/` | https://www.w3.org/2013/NQuadsTests/ |
| `tests/testdata/w3c_sparql11/` | https://www.w3.org/2009/sparql/docs/tests/ |

Copyright © World Wide Web Consortium (W3C) and its contributors.  
License: http://www.w3.org/Consortium/Legal/2008/04-testsuite-license  
BSD variant: http://www.w3.org/Consortium/Legal/2008/03-bsd-license

The W3C Test Suite License requires that any redistribution include:
- a link to the original W3C document,
- the original copyright notice, and
- the status of the W3C document.

---

## OWL Time Ontology

**File:** `tests/testdata/owl-time.ttl`  
**Source:** https://www.w3.org/TR/owl-time/  
**Copyright:** Copyright © 2006–2017 W3C, OGC.  
**License:** Creative Commons Attribution 4.0 International (CC BY 4.0)  
https://creativecommons.org/licenses/by/4.0/

---

## PROV-O: The PROV Ontology

**File:** `tests/testdata/prov-o.ttl`  
**Source:** https://www.w3.org/TR/prov-o/ (W3C Recommendation 2013-04-30)  
**Copyright:** Copyright © 2013 W3C® (MIT, ERCIM, Keio, Beihang). All Rights Reserved.  
**License:** W3C Software and Document License  
https://www.w3.org/Consortium/Legal/2015/copyright-software-and-document

---

## DCMI Metadata Terms

**File:** `tests/testdata/dcterms.ttl`  
**Source:** https://www.dublincore.org/specifications/dublin-core/dcmi-terms/  
**Copyright:** Copyright © Dublin Core Metadata Initiative (DCMI). All Rights Reserved.  
**License:** Creative Commons Attribution 4.0 International (CC BY 4.0)  
https://creativecommons.org/licenses/by/4.0/

---

## ISO 15926 Part 14 (LIS-14) Core Ontology

**File:** `tests/testdata/LIS-14.ttl`  
**Source:** http://rds.posccaesar.org/ontology/lis14/ont/core  
**Copyright:** Copyright POSC Caesar Association  
**License:** Creative Commons Attribution-ShareAlike 4.0 International (CC BY-SA 4.0)  
https://creativecommons.org/licenses/by-sa/4.0/

The share-alike clause of CC BY-SA 4.0 applies to derivatives of LIS-14 itself.
This file is used only as a test fixture and is not incorporated into the
library code or its outputs.

---

## Gene Ontology (GO)

**Files:** `tests/testdata/go.ttl`, `tests/testdata/go.owl.xml`  
*(These files are excluded from version control via `.gitignore` and must be
downloaded separately via `scripts/download_test_ontologies.sh`.)*  
**Source:** https://geneontology.org/ / https://purl.obolibrary.org/obo/go.owl  
**License:** Creative Commons Attribution 4.0 International (CC BY 4.0)  
https://creativecommons.org/licenses/by/4.0/

---

## SHACL Specification Examples

**Files:** `tests/testdata/shacl_*.ttl`  
These files are small data graphs written to exercise the SHACL validator,
based on examples in the W3C SHACL specification. Each file carries a comment
header citing the specific section and URL from:  
https://www.w3.org/TR/shacl/

Copyright © 2017 W3C® (MIT, ERCIM, Keio, Beihang).  
Used under the W3C Document License:  
https://www.w3.org/Consortium/Legal/2015/doc-license
