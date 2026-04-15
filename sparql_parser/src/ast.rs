use dag_rdf::GraphElement;

#[derive(Debug, Clone, PartialEq)]
pub enum Query {
    Select {
        projection: Vec<ProjectionElement>,
        where_clause: Vec<QueryComponent>,
        group_by: Vec<Expression>,
        having: Vec<Expression>,
        order_by: Vec<OrderCondition>,
        limit: Option<u64>,
        offset: Option<u64>,
        distinct: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProjectionElement {
    Variable(String),
    Expression(Expression, String), // Expression and alias
    Star,
}

#[derive(Debug, Clone, PartialEq)]
pub enum QueryComponent {
    BGP(Vec<TriplePattern>),
    Optional(Vec<QueryComponent>),
    Union(Vec<QueryComponent>, Vec<QueryComponent>),
    Filter(Expression),
    Bind(Expression, String),
    Values(Vec<String>, Vec<Vec<Option<GraphElement>>>),
    Minus(Vec<QueryComponent>),
    Graph(Term, Vec<QueryComponent>),
    Service(Term, Vec<QueryComponent>, bool), // bool is silent
}

#[derive(Debug, Clone, PartialEq)]
pub struct TriplePattern {
    pub subject: Term,
    pub predicate: Term,
    pub object: Term,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Term {
    Variable(String),
    Constant(GraphElement),
}

#[derive(Debug, Clone, PartialEq)]
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

#[derive(Debug, Clone, PartialEq)]
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

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    Not,
    Plus,
    Minus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Aggregate {
    Count(Box<Expression>, bool), // bool is distinct
    Sum(Box<Expression>, bool),
    Avg(Box<Expression>, bool),
    Min(Box<Expression>, bool),
    Max(Box<Expression>, bool),
    Sample(Box<Expression>, bool),
    GroupConcat(Box<Expression>, String, bool), // String is separator
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderCondition {
    pub expression: Expression,
    pub ascending: bool,
}
