/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Criterion benchmarks for LUBM-scale incremental Datalog maintenance.
//!
//! Data is generated **synthetically** using the LUBM UnivBench vocabulary —
//! no external download is required.
//!
//! Run all LUBM benchmarks:
//! ```bash
//! cargo bench --bench lubm
//! ```
//!
//! Run a specific group:
//! ```bash
//! cargo bench --bench lubm -- bf_vs_full_remat
//! cargo bench --bench lubm -- memory_overhead
//! cargo bench --bench lubm -- insert_seminaive
//! ```
//!
//! Verify that the benchmarks compile and run without panicking (test mode):
//! ```bash
//! cargo bench --bench lubm -- --test
//! ```
//!
//! Related issue: [#111](https://github.com/daghovland/rdf-datalog/issues/111)

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use dag_rdf::{
    DEFAULT_GRAPH_ELEMENT_ID, Datastore, GraphElement, IriReference, Quad, QuadPattern, QuadTable,
    RdfResource, Term,
};
use datalog::{IncrementalReasoner, Rule, RuleAtom, RuleHead, evaluate_rules};

// ── LUBM vocabulary namespace ─────────────────────────────────────────────────

const LUBM_NS: &str = "http://swat.cse.lehigh.edu/onto/univ-bench.owl#";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const EXAMPLE_NS: &str = "http://lubm.example.org/";

// ── Interned vocabulary IDs ───────────────────────────────────────────────────

/// Interned IDs for the LUBM vocabulary, captured during Datastore construction.
struct LubmVocab {
    g: u32,
    rdf_type: u32,
    // classes
    univ_cls: u32,
    dept_cls: u32,
    professor_cls: u32,
    full_professor: u32,
    assoc_professor: u32,
    asst_professor: u32,
    lecturer: u32,
    student_cls: u32,
    grad_student: u32,
    ugrad_student: u32,
    course_cls: u32,
    grad_course_cls: u32,
    // properties
    member_of: u32,
    sub_org_of: u32,
    works_for: u32,
    takes_course: u32,
    teacher_of: u32,
    advisor: u32,
    doctoral_from: u32,
    head_of: u32,
}

fn lubm_iri(local: &str) -> String {
    format!("{LUBM_NS}{local}")
}

fn intern(ds: &mut Datastore, iri: &str) -> u32 {
    ds.add_resource(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
        iri.to_string(),
    ))))
}

fn add_type(ds: &mut Datastore, g: u32, rdf_type: u32, subject: u32, class: u32) {
    ds.named_graphs.add_quad(Quad {
        triple_id: g,
        subject,
        predicate: rdf_type,
        obj: class,
    });
}

fn add_prop(ds: &mut Datastore, g: u32, subject: u32, predicate: u32, object: u32) {
    ds.named_graphs.add_quad(Quad {
        triple_id: g,
        subject,
        predicate,
        obj: object,
    });
}

fn intern_vocab(ds: &mut Datastore) -> LubmVocab {
    LubmVocab {
        g: DEFAULT_GRAPH_ELEMENT_ID,
        rdf_type: intern(ds, RDF_TYPE),
        univ_cls: intern(ds, &lubm_iri("University")),
        dept_cls: intern(ds, &lubm_iri("Department")),
        professor_cls: intern(ds, &lubm_iri("Professor")),
        full_professor: intern(ds, &lubm_iri("FullProfessor")),
        assoc_professor: intern(ds, &lubm_iri("AssociateProfessor")),
        asst_professor: intern(ds, &lubm_iri("AssistantProfessor")),
        lecturer: intern(ds, &lubm_iri("Lecturer")),
        student_cls: intern(ds, &lubm_iri("Student")),
        grad_student: intern(ds, &lubm_iri("GraduateStudent")),
        ugrad_student: intern(ds, &lubm_iri("UndergraduateStudent")),
        course_cls: intern(ds, &lubm_iri("Course")),
        grad_course_cls: intern(ds, &lubm_iri("GraduateCourse")),
        member_of: intern(ds, &lubm_iri("memberOf")),
        sub_org_of: intern(ds, &lubm_iri("subOrganizationOf")),
        works_for: intern(ds, &lubm_iri("worksFor")),
        takes_course: intern(ds, &lubm_iri("takesCourse")),
        teacher_of: intern(ds, &lubm_iri("teacherOf")),
        advisor: intern(ds, &lubm_iri("advisor")),
        doctoral_from: intern(ds, &lubm_iri("doctoralDegreeFrom")),
        head_of: intern(ds, &lubm_iri("headOf")),
    }
}

// ── Synthetic ABox generator ──────────────────────────────────────────────────

/// Synthetic LUBM ABox generator.
///
/// `scale` controls corpus size:
/// - scale 1  → ~100 k triples (5 universities)
/// - scale 5  → ~500 k triples
/// - scale 10 → ~1 M triples
struct LubmGenerator {
    scale: usize,
}

impl LubmGenerator {
    fn new(scale: usize) -> Self {
        LubmGenerator { scale }
    }

    /// Generate a synthetic LUBM ABox of approximately `scale × 100 000` triples.
    ///
    /// Layout per scale unit: 5 universities × 4 departments × professors
    /// (8 FullProfessors, 4 AssociateProfessors, 4 AssistantProfessors, 2 Lecturers)
    /// × (15 graduate + 20 undergraduate students per FullProfessor).
    fn generate(&self) -> (Datastore, LubmVocab) {
        let n_universities = 5 * self.scale;
        let estimated_quads = (n_universities * 20_000) as u32;
        let mut ds = Datastore::new(estimated_quads);
        let v = intern_vocab(&mut ds);

        for u in 0..n_universities {
            let univ_iri = format!("{EXAMPLE_NS}university{u}");
            let univ = intern(&mut ds, &univ_iri);
            add_type(&mut ds, v.g, v.rdf_type, univ, v.univ_cls);

            for d in 0..4_usize {
                let dept_iri = format!("{EXAMPLE_NS}university{u}/department{d}");
                let dept = intern(&mut ds, &dept_iri);
                add_type(&mut ds, v.g, v.rdf_type, dept, v.dept_cls);
                add_prop(&mut ds, v.g, dept, v.sub_org_of, univ);

                // Head of department
                let head_iri = format!("{EXAMPLE_NS}university{u}/department{d}/head");
                let head = intern(&mut ds, &head_iri);
                add_type(&mut ds, v.g, v.rdf_type, head, v.full_professor);
                add_prop(&mut ds, v.g, head, v.works_for, dept);
                add_prop(&mut ds, v.g, head, v.member_of, dept);
                add_prop(&mut ds, v.g, dept, v.head_of, head);
                add_prop(&mut ds, v.g, head, v.doctoral_from, univ);

                // FullProfessors (8 per department)
                // Students per FullProfessor: 10 grad + 15 undergrad
                for fp in 0..8_usize {
                    let prof_iri = format!("{EXAMPLE_NS}university{u}/department{d}/fullprof{fp}");
                    let prof = intern(&mut ds, &prof_iri);
                    add_type(&mut ds, v.g, v.rdf_type, prof, v.full_professor);
                    add_prop(&mut ds, v.g, prof, v.works_for, dept);
                    add_prop(&mut ds, v.g, prof, v.member_of, dept);
                    add_prop(&mut ds, v.g, prof, v.doctoral_from, univ);
                    Self::add_students(&mut ds, &v, u, d, "fp", fp, prof, dept, 10, 15);
                    Self::add_courses(&mut ds, &v, u, d, "fp", fp, prof, 3);
                }

                // AssociateProfessors (4 per department)
                for ap in 0..4_usize {
                    let prof_iri = format!("{EXAMPLE_NS}university{u}/department{d}/assocprof{ap}");
                    let prof = intern(&mut ds, &prof_iri);
                    add_type(&mut ds, v.g, v.rdf_type, prof, v.assoc_professor);
                    add_prop(&mut ds, v.g, prof, v.works_for, dept);
                    add_prop(&mut ds, v.g, prof, v.member_of, dept);
                    add_prop(&mut ds, v.g, prof, v.doctoral_from, univ);
                    Self::add_students(&mut ds, &v, u, d, "ap", ap, prof, dept, 4, 8);
                    Self::add_courses(&mut ds, &v, u, d, "ap", ap, prof, 2);
                }

                // AssistantProfessors (4 per department)
                for asp in 0..4_usize {
                    let prof_iri = format!("{EXAMPLE_NS}university{u}/department{d}/astprof{asp}");
                    let prof = intern(&mut ds, &prof_iri);
                    add_type(&mut ds, v.g, v.rdf_type, prof, v.asst_professor);
                    add_prop(&mut ds, v.g, prof, v.works_for, dept);
                    add_prop(&mut ds, v.g, prof, v.member_of, dept);
                    add_prop(&mut ds, v.g, prof, v.doctoral_from, univ);
                    Self::add_students(&mut ds, &v, u, d, "asp", asp, prof, dept, 3, 6);
                    Self::add_courses(&mut ds, &v, u, d, "asp", asp, prof, 2);
                }

                // Lecturers (2 per department)
                for lec in 0..2_usize {
                    let prof_iri = format!("{EXAMPLE_NS}university{u}/department{d}/lecturer{lec}");
                    let prof = intern(&mut ds, &prof_iri);
                    add_type(&mut ds, v.g, v.rdf_type, prof, v.lecturer);
                    add_prop(&mut ds, v.g, prof, v.works_for, dept);
                    add_prop(&mut ds, v.g, prof, v.member_of, dept);
                    Self::add_students(&mut ds, &v, u, d, "lec", lec, prof, dept, 0, 5);
                    Self::add_courses(&mut ds, &v, u, d, "lec", lec, prof, 1);
                }
            }
        }

        (ds, v)
    }

    #[allow(clippy::too_many_arguments)]
    fn add_students(
        ds: &mut Datastore,
        v: &LubmVocab,
        u: usize,
        d: usize,
        prof_kind: &str,
        prof_idx: usize,
        prof_id: u32,
        dept_id: u32,
        n_grad: usize,
        n_ugrad: usize,
    ) {
        for s in 0..n_grad {
            let student_iri = format!(
                "{EXAMPLE_NS}university{u}/department{d}/{prof_kind}{prof_idx}/gradstudent{s}"
            );
            let student = intern(ds, &student_iri);
            add_type(ds, v.g, v.rdf_type, student, v.grad_student);
            add_prop(ds, v.g, student, v.member_of, dept_id);
            add_prop(ds, v.g, student, v.advisor, prof_id);
            // grad course
            let course_iri = format!(
                "{EXAMPLE_NS}university{u}/department{d}/{prof_kind}{prof_idx}/gradcourse{s}"
            );
            let course = intern(ds, &course_iri);
            add_type(ds, v.g, v.rdf_type, course, v.grad_course_cls);
            add_prop(ds, v.g, student, v.takes_course, course);
        }
        for s in 0..n_ugrad {
            let student_iri = format!(
                "{EXAMPLE_NS}university{u}/department{d}/{prof_kind}{prof_idx}/ugradstudent{s}"
            );
            let student = intern(ds, &student_iri);
            add_type(ds, v.g, v.rdf_type, student, v.ugrad_student);
            add_prop(ds, v.g, student, v.member_of, dept_id);
            let course_iri =
                format!("{EXAMPLE_NS}university{u}/department{d}/{prof_kind}{prof_idx}/course{s}");
            let course = intern(ds, &course_iri);
            add_type(ds, v.g, v.rdf_type, course, v.course_cls);
            add_prop(ds, v.g, student, v.takes_course, course);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn add_courses(
        ds: &mut Datastore,
        v: &LubmVocab,
        u: usize,
        d: usize,
        prof_kind: &str,
        prof_idx: usize,
        prof_id: u32,
        n_courses: usize,
    ) {
        for c in 0..n_courses {
            let course_iri =
                format!("{EXAMPLE_NS}university{u}/department{d}/{prof_kind}{prof_idx}/taught{c}");
            let course = intern(ds, &course_iri);
            add_type(ds, v.g, v.rdf_type, course, v.course_cls);
            add_prop(ds, v.g, prof_id, v.teacher_of, course);
        }
    }

    /// Build the LUBM class-hierarchy Datalog rules.
    ///
    /// These encode OWL-RL subClassOf/subPropertyOf chains that produce
    /// rdf:type and memberOf inferences from the ABox.
    fn make_rules(&self, v: &LubmVocab) -> Vec<Rule> {
        let g = v.g;
        let x = || Term::Variable("x".to_string());
        let y = || Term::Variable("y".to_string());

        let type_rule = |sub_cls: u32, super_cls: u32| Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: x(),
                predicate: Term::Resource(v.rdf_type),
                object: Term::Resource(super_cls),
            }),
            body: vec![RuleAtom::PositivePattern(QuadPattern {
                graph: Term::Resource(g),
                subject: x(),
                predicate: Term::Resource(v.rdf_type),
                object: Term::Resource(sub_cls),
            })],
        };

        let prop_rule = |sub_pred: u32, super_pred: u32| Rule {
            head: RuleHead::NormalHead(QuadPattern {
                graph: Term::Resource(g),
                subject: x(),
                predicate: Term::Resource(super_pred),
                object: y(),
            }),
            body: vec![RuleAtom::PositivePattern(QuadPattern {
                graph: Term::Resource(g),
                subject: x(),
                predicate: Term::Resource(sub_pred),
                object: y(),
            })],
        };

        vec![
            // FullProfessor → Professor
            type_rule(v.full_professor, v.professor_cls),
            // AssociateProfessor → Professor
            type_rule(v.assoc_professor, v.professor_cls),
            // AssistantProfessor → Professor
            type_rule(v.asst_professor, v.professor_cls),
            // Lecturer → Professor
            type_rule(v.lecturer, v.professor_cls),
            // GraduateStudent → Student
            type_rule(v.grad_student, v.student_cls),
            // UndergraduateStudent → Student
            type_rule(v.ugrad_student, v.student_cls),
            // GraduateCourse → Course
            type_rule(v.grad_course_cls, v.course_cls),
            // worksFor → memberOf  (subPropertyOf)
            prop_rule(v.works_for, v.member_of),
        ]
    }

    /// Pick the first `delta_count` extensional quads as a deletion delta.
    fn pick_deletions(ds: &Datastore, delta_count: usize) -> Vec<Quad> {
        ds.named_graphs
            .extensional_quads()
            .take(delta_count)
            .collect()
    }
}

// ── Memory helper ─────────────────────────────────────────────────────────────

/// Read VmRSS (resident set size) from `/proc/self/status` in kilobytes.
/// Returns 0 on non-Linux platforms or parse failure.
fn read_vm_rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|n| n.parse().ok())
        })
        .unwrap_or(0)
}

// ── Benchmark: memory overhead of DerivedFrom index ──────────────────────────

fn bench_memory_overhead(c: &mut Criterion) {
    // Emit RSS measurement once, outside the Criterion loop (scale=1 only).
    // For larger scales run: cargo bench --bench lubm -- memory_overhead
    {
        let lubm = LubmGenerator::new(1);
        let before = read_vm_rss_kb();
        let (mut ds, v) = lubm.generate();
        let rules = lubm.make_rules(&v);
        let _reasoner = IncrementalReasoner::new(rules, &mut ds);
        let after = read_vm_rss_kb();
        eprintln!(
            "[lubm/memory_overhead] scale=1: quads={} derived={} RSS_delta={}KB",
            ds.named_graphs.quad_count,
            ds.named_graphs.intensional_quads_iter().count(),
            after.saturating_sub(before),
        );
    }

    // Criterion timing: full materialisation at scale 1.
    let lubm = LubmGenerator::new(1);
    c.bench_function("lubm/memory_overhead/scale1", |b| {
        b.iter_batched(
            || lubm.generate(),
            |(mut ds, v)| {
                let rules = lubm.make_rules(&v);
                let reasoner = IncrementalReasoner::new(rules, &mut ds);
                (ds, reasoner)
            },
            BatchSize::LargeInput,
        );
    });
}

// ── Benchmark: BF single delete latency ───────────────────────────────────────

fn bench_bf_single_delete(c: &mut Criterion) {
    let lubm = LubmGenerator::new(1);
    c.bench_function("lubm/bf_single_delete/scale1", |b| {
        b.iter_batched(
            || {
                let (mut ds, v) = lubm.generate();
                let rules = lubm.make_rules(&v);
                let reasoner = IncrementalReasoner::new(rules, &mut ds);
                let to_delete = LubmGenerator::pick_deletions(&ds, 1);
                (ds, reasoner, to_delete)
            },
            |(mut ds, mut reasoner, to_delete)| {
                reasoner.apply_deletions(&mut ds, &to_delete);
            },
            BatchSize::LargeInput,
        );
    });
}

// ── Benchmark: BF delete vs full re-materialisation ──────────────────────────
//
// Default scale = 1 (~37k extensional quads) so that `--test` mode completes
// in a few seconds.  To benchmark at larger scales run:
//
//   LUBM_BENCH_SCALE=5  cargo bench --bench lubm -- bf_vs_full_remat
//   LUBM_BENCH_SCALE=10 cargo bench --bench lubm -- bf_vs_full_remat

fn bench_scale() -> usize {
    std::env::var("LUBM_BENCH_SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1)
}

/// Delete percentages to test.
///
/// Default = [1, 5, 10, 20] for a full tipping-point study.
/// Set LUBM_BENCH_PCTS to override, e.g. LUBM_BENCH_PCTS=1 for a quick smoke test.
///
/// Note: remove_quad is O(n) per call so 10%+ deletions on large stores are slow.
/// For the full study:
///   cargo bench --bench lubm -- bf_vs_full_remat
/// For a quick check:
///   LUBM_BENCH_PCTS=1 cargo bench --bench lubm -- bf_vs_full_remat
fn bench_pcts() -> Vec<usize> {
    std::env::var("LUBM_BENCH_PCTS")
        .ok()
        .map(|s| s.split(',').filter_map(|p| p.trim().parse().ok()).collect())
        .unwrap_or_else(|| vec![1, 5, 10, 20])
}

fn bench_bf_vs_full_remat(c: &mut Criterion) {
    let mut group = c.benchmark_group("lubm/bf_vs_full_remat");

    let scale = bench_scale();
    {
        let lubm = LubmGenerator::new(scale);

        for pct in bench_pcts() {
            // ── BF variant ──────────────────────────────────────────────────
            group.bench_with_input(
                BenchmarkId::new(format!("bf/scale{scale}"), pct),
                &pct,
                |b, &pct| {
                    b.iter_batched(
                        || {
                            let (mut ds, v) = lubm.generate();
                            let rules = lubm.make_rules(&v);
                            let reasoner = IncrementalReasoner::new(rules, &mut ds);
                            let n_ext = ds.named_graphs.extensional_quads().count();
                            let delta_count = std::cmp::max(1, n_ext * pct / 100);
                            let to_delete = LubmGenerator::pick_deletions(&ds, delta_count);
                            (ds, reasoner, to_delete)
                        },
                        |(mut ds, mut reasoner, to_delete)| {
                            reasoner.apply_deletions(&mut ds, &to_delete);
                        },
                        BatchSize::LargeInput,
                    );
                },
            );

            // ── Full re-mat variant ─────────────────────────────────────────
            group.bench_with_input(
                BenchmarkId::new(format!("full_remat/scale{scale}"), pct),
                &pct,
                |b, &pct| {
                    b.iter_batched(
                        || {
                            let (mut ds, v) = lubm.generate();
                            let rules = lubm.make_rules(&v);
                            // Materialise so intensional_quads is populated.
                            let _reasoner = IncrementalReasoner::new(rules.clone(), &mut ds);
                            let n_ext = ds.named_graphs.extensional_quads().count();
                            let delta_count = std::cmp::max(1, n_ext * pct / 100);
                            let to_delete = LubmGenerator::pick_deletions(&ds, delta_count);
                            (ds, rules, to_delete)
                        },
                        |(mut ds, rules, to_delete)| {
                            // Remove base facts.
                            for q in &to_delete {
                                ds.named_graphs.remove_quad(*q);
                            }
                            // Snapshot surviving extensional quads.
                            let surviving: Vec<Quad> =
                                ds.named_graphs.extensional_quads().collect();
                            let hint = surviving.len() as u32;
                            // Reset the quad table to only extensional facts.
                            ds.named_graphs = QuadTable::new(hint);
                            for q in surviving {
                                ds.named_graphs.add_quad(q);
                            }
                            // Full re-materialisation from scratch.
                            evaluate_rules(rules, &mut ds);
                        },
                        BatchSize::LargeInput,
                    );
                },
            );
        }
    }

    group.finish();
}

// ── Benchmark: semi-naive insert latency ─────────────────────────────────────
//
// Uses LUBM_BENCH_SCALE (default=1) to control the base corpus size.

fn bench_insert_seminaive(c: &mut Criterion) {
    let mut group = c.benchmark_group("lubm/insert_seminaive");

    let scale = bench_scale();
    {
        // Base corpus: half the scale, already materialised.
        // Insert delta: ~1% of base extensional quads drawn from the larger corpus.
        let base_scale = std::cmp::max(1, scale / 2);
        let base_lubm = LubmGenerator::new(base_scale);
        let full_lubm = LubmGenerator::new(scale);

        group.bench_with_input(BenchmarkId::new("scale", scale), &scale, |b, _| {
            b.iter_batched(
                || {
                    // Build and materialise the base store.
                    let (mut base_ds, base_v) = base_lubm.generate();
                    let base_rules = base_lubm.make_rules(&base_v);
                    let reasoner = IncrementalReasoner::new(base_rules, &mut base_ds);

                    // Build the full corpus and collect extra extensional quads.
                    let (full_ds, _) = full_lubm.generate();
                    let base_set: std::collections::HashSet<Quad> =
                        base_ds.named_graphs.extensional_quads().collect();
                    let one_pct =
                        std::cmp::max(1, base_ds.named_graphs.extensional_quads().count() / 100);
                    let insert_delta: Vec<Quad> = full_ds
                        .named_graphs
                        .extensional_quads()
                        .filter(|q| !base_set.contains(q))
                        .take(one_pct)
                        .collect();

                    (base_ds, reasoner, insert_delta)
                },
                |(mut ds, mut reasoner, inserts)| {
                    reasoner.apply_insertions(&mut ds, &inserts);
                },
                BatchSize::LargeInput,
            );
        });
    }

    group.finish();
}

// ── Criterion wiring ──────────────────────────────────────────────────────────

criterion_group!(
    lubm_benches,
    bench_memory_overhead,
    bench_bf_single_delete,
    bench_bf_vs_full_remat,
    bench_insert_seminaive,
);
criterion_main!(lubm_benches);
