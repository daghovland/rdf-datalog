use dag_rdf::GraphElement;

/// A SPARQL property path expression (predicate position in triple patterns).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PropertyPath {
    /// Single named property (IRI or 'a')
    Iri(GraphElement),
    /// p1/p2/... — sequence
    Sequence(Vec<PropertyPath>),
    /// p1|p2 — alternative (union)
    Alternative(Box<PropertyPath>, Box<PropertyPath>),
    /// ^p — inverse
    Inverse(Box<PropertyPath>),
    /// p* — zero or more hops
    ZeroOrMore(Box<PropertyPath>),
    /// p+ — one or more hops
    OneOrMore(Box<PropertyPath>),
    /// p? — zero or one hop
    ZeroOrOne(Box<PropertyPath>),
    /// p{n}, p{n,m}, p{n,}, p{,m} — bounded/unbounded repetition.
    ///
    /// `min` is the minimum repeat count; `max` is `Some(m)` for a bounded
    /// upper limit (`{n,m}`/`{n}` where `m == n`) or `None` for an unbounded
    /// lower-bounded range (`{n,}`). `{,m}` is represented with `min == 0`.
    ///
    /// Unlike `ZeroOrMore`/`OneOrMore` (which use arbitrary-length-path /
    /// fixed-point reachability semantics — one solution per reachable
    /// subject/object pair), bounded repetition follows ordinary sequence
    /// (join) semantics: each distinct walk of an in-range length is its own
    /// solution, so duplicate rows are expected when multiple walks of the
    /// same length connect the same endpoints (see W3C property-path tests
    /// pp20/pp22/pp24/pp26/pp27/pp29, tracked in
    /// <https://github.com/daghovland/rdf-datalog/issues/203>).
    Repeat(Box<PropertyPath>, usize, Option<usize>),
    /// !(p1|p2|...) — negated property set
    NegatedSet(Vec<GraphElement>),
}

/// A dataset clause from `FROM` or `FROM NAMED`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DatasetClause {
    /// `FROM <iri>` — contributes to the default graph.
    Default(GraphElement),
    /// `FROM NAMED <iri>` — adds to the set of accessible named graphs.
    Named(GraphElement),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Query {
    Select {
        projection: Vec<ProjectionElement>,
        dataset: Vec<DatasetClause>,
        where_clause: Vec<QueryComponent>,
        group_by: Vec<Expression>,
        having: Vec<Expression>,
        order_by: Vec<OrderCondition>,
        limit: Option<u64>,
        offset: Option<u64>,
        distinct: bool,
    },
    Ask {
        dataset: Vec<DatasetClause>,
        where_clause: Vec<QueryComponent>,
    },
    Construct {
        /// Template triple patterns; empty means short form (use WHERE BGPs as template).
        template: Vec<TriplePattern>,
        dataset: Vec<DatasetClause>,
        where_clause: Vec<QueryComponent>,
    },
    Describe {
        /// Resources to describe: IRIs or variables.  Empty means `DESCRIBE *`.
        resources: Vec<Term>,
        dataset: Vec<DatasetClause>,
        where_clause: Vec<QueryComponent>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ProjectionElement {
    Variable(String),
    Expression(Expression, String), // Expression and alias
    Star,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum QueryComponent {
    BGP(Vec<TriplePattern>),
    PathPattern(Term, Box<PropertyPath>, Term),
    /// `{ SELECT ... }` embedded inside a group graph pattern.
    Subquery(Box<Query>),
    Optional(Vec<QueryComponent>),
    Union(Vec<QueryComponent>, Vec<QueryComponent>),
    Filter(Expression),
    Bind(Expression, String),
    Values(Vec<String>, Vec<Vec<Option<GraphElement>>>),
    Minus(Vec<QueryComponent>),
    Graph(Term, Vec<QueryComponent>),
    Service(Term, Vec<QueryComponent>, bool), // bool is silent
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TriplePattern {
    pub subject: Term,
    pub predicate: Term,
    pub object: Term,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Term {
    Variable(String),
    Constant(GraphElement),
    /// An RDF 1.2 triple term pattern: `<<( subject predicate object )>>`.
    ///
    /// Only supported in the subject position of the outer triple pattern by
    /// the executor (see `sparql_parser::execute`); appearing elsewhere is a
    /// parse-level accommodation for forward compatibility but currently
    /// yields no matches. Tracked in epic
    /// [#143](https://github.com/daghovland/rdf-datalog/issues/143), phase R3
    /// ([#146](https://github.com/daghovland/rdf-datalog/issues/146)).
    /// Object-position support and datalog rule integration are deferred to a
    /// future "R6" issue — see `docs/plans/RDF12_PLAN.md`.
    TripleTerm(Box<TriplePattern>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Expression {
    Variable(String),
    Constant(GraphElement),
    Binary(Box<Expression>, BinaryOp, Box<Expression>),
    Unary(UnaryOp, Box<Expression>),
    FunctionCall(String, Vec<Expression>),
    Aggregate(Aggregate),
    Exists(Vec<QueryComponent>),
    NotExists(Vec<QueryComponent>),
    In(Box<Expression>, Vec<Expression>),
    NotIn(Box<Expression>, Vec<Expression>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum UnaryOp {
    Not,
    Plus,
    Minus,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Aggregate {
    CountStar,                    // COUNT(*)
    Count(Box<Expression>, bool), // COUNT(DISTINCT? expr), bool = distinct
    Sum(Box<Expression>, bool),
    Avg(Box<Expression>, bool),
    Min(Box<Expression>, bool),
    Max(Box<Expression>, bool),
    Sample(Box<Expression>, bool),
    GroupConcat(Box<Expression>, String, bool), // String = separator, bool = distinct
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OrderCondition {
    pub expression: Expression,
    pub ascending: bool,
}
