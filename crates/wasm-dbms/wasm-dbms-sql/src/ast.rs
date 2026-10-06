//! Abstract syntax tree of the supported SQL dialect.
//!
//! [`parse`](crate::parse) produces these types. They describe the statement
//! exactly as written: names are not checked against a schema and literals are
//! not converted to column types yet. The grammar they model is documented in
//! the SQL reference of the wasm-dbms book.

/// A parsed SQL statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Statement {
    /// `SELECT ...`
    ///
    /// Boxed because a `SELECT` is much larger than the other statements.
    Select(Box<Select>),
    /// `INSERT INTO ...`
    Insert(Insert),
    /// `UPDATE ...`
    Update(Update),
    /// `DELETE FROM ...`
    Delete(Delete),
    /// `BEGIN [TRANSACTION]`
    Begin,
    /// `COMMIT [TRANSACTION]`
    Commit,
    /// `ROLLBACK [TRANSACTION]`
    Rollback,
}

/// A `SELECT` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Select {
    /// Whether `DISTINCT` follows `SELECT`.
    pub distinct: bool,
    /// The select list.
    pub projection: Projection,
    /// The `FROM` table.
    pub from: TableRef,
    /// The `JOIN` clauses, in source order.
    pub joins: Vec<JoinClause>,
    /// The `WHERE` condition.
    pub filter: Option<Condition>,
    /// The `GROUP BY` columns.
    pub group_by: Vec<ColumnRef>,
    /// The `HAVING` condition.
    pub having: Option<Condition>,
    /// The `ORDER BY` items, most significant first.
    pub order_by: Vec<OrderItem>,
    /// The `LIMIT` row count.
    pub limit: Option<RowCount>,
    /// The `OFFSET` row count.
    pub offset: Option<RowCount>,
}

/// The select list of a [`Select`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Projection {
    /// `*`: every column.
    All,
    /// An explicit list of columns and aggregates.
    Items(Vec<SelectItem>),
}

/// One entry of an explicit select list: `operand [AS alias]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectItem {
    /// The selected column or aggregate.
    pub operand: Operand,
    /// The output name given with `AS`.
    pub alias: Option<String>,
}

/// Something that yields a value per row or per group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operand {
    /// A column reference.
    Column(ColumnRef),
    /// An aggregate function call.
    Aggregate(Aggregate),
}

/// A column name, optionally qualified: `column` or `table.column`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnRef {
    /// The table name or table alias before the dot.
    pub table: Option<String>,
    /// The column name.
    pub column: String,
}

/// An aggregate function call such as `COUNT(*)` or `SUM(price)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aggregate {
    /// The aggregate function.
    pub function: AggregateKind,
    /// The aggregated column; `None` only for `COUNT(*)`.
    pub column: Option<ColumnRef>,
}

/// The supported aggregate functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggregateKind {
    Count,
    Sum,
    Avg,
    Min,
    Max,
}

/// A table in `FROM` or `JOIN`: `name [[AS] alias]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRef {
    /// The table name.
    pub name: String,
    /// The alias that replaces the table name as column qualifier.
    pub alias: Option<String>,
}

/// A `JOIN table ON left = right` clause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinClause {
    /// The join type.
    pub kind: JoinKind,
    /// The joined table.
    pub table: TableRef,
    /// The column written on the left of `=`.
    pub left: ColumnRef,
    /// The column written on the right of `=`.
    pub right: ColumnRef,
}

/// The supported join types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinKind {
    /// `JOIN` or `INNER JOIN`
    Inner,
    /// `LEFT [OUTER] JOIN`
    Left,
    /// `RIGHT [OUTER] JOIN`
    Right,
    /// `FULL [OUTER] JOIN`
    Full,
}

/// A boolean condition of a `WHERE` or `HAVING` clause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Condition {
    /// `left AND right`
    And(Box<Condition>, Box<Condition>),
    /// `left OR right`
    Or(Box<Condition>, Box<Condition>),
    /// `NOT inner`
    Not(Box<Condition>),
    /// `operand op value`
    Compare {
        operand: Operand,
        op: CompareOp,
        value: ValueExpr,
    },
    /// `operand [NOT] IN (values)`
    In {
        operand: Operand,
        values: Vec<ValueExpr>,
        negated: bool,
    },
    /// `operand [NOT] LIKE pattern`
    Like {
        operand: Operand,
        pattern: ValueExpr,
        negated: bool,
    },
    /// `operand IS [NOT] NULL`
    IsNull { operand: Operand, negated: bool },
}

/// A comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    /// `=`
    Eq,
    /// `!=` or `<>`
    NotEq,
    /// `<`
    Lt,
    /// `<=`
    LtEq,
    /// `>`
    Gt,
    /// `>=`
    GtEq,
}

/// A value written in the statement: a literal or a `?` placeholder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueExpr {
    /// A literal value.
    Literal(Literal),
    /// The `?` placeholder with this 0-based position in the statement.
    Parameter(usize),
}

/// A literal value, before conversion to a column type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Literal {
    /// An integer, with its sign applied.
    Integer(i128),
    /// A decimal number as written (`[-]digits.digits`).
    Float(String),
    /// The content of a single-quoted string.
    String(String),
    /// `TRUE` or `FALSE`
    Boolean(bool),
    /// `NULL`
    Null,
}

/// One `ORDER BY` entry: `operand [ASC | DESC]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderItem {
    /// The column, select-list alias, or aggregate to sort by.
    pub operand: Operand,
    /// Whether `DESC` was given.
    pub descending: bool,
}

/// The argument of `LIMIT` or `OFFSET`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowCount {
    /// A non-negative integer literal.
    Value(u64),
    /// The `?` placeholder with this 0-based position in the statement.
    Parameter(usize),
}

/// An `INSERT INTO table (columns) VALUES (values)` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Insert {
    /// The target table.
    pub table: String,
    /// The listed columns.
    pub columns: Vec<String>,
    /// One value per listed column, in the same order.
    pub values: Vec<ValueExpr>,
}

/// An `UPDATE table SET assignments WHERE condition` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    /// The target table.
    pub table: String,
    /// The `SET` assignments.
    pub assignments: Vec<Assignment>,
    /// The mandatory `WHERE` condition.
    pub filter: Condition,
}

/// One `column = value` entry of an `UPDATE ... SET` list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    /// The column to write.
    pub column: String,
    /// The new value.
    pub value: ValueExpr,
}

/// A `DELETE FROM table WHERE condition [CASCADE | RESTRICT]` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delete {
    /// The target table.
    pub table: String,
    /// The mandatory `WHERE` condition.
    pub filter: Condition,
    /// Whether `CASCADE` was given; `RESTRICT` is the default.
    pub cascade: bool,
}
