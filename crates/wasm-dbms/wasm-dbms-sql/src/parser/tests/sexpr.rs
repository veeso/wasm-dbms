//! Renders a parsed statement as an S-expression.
//!
//! The fixture files under `tests/fixtures/parser` state the expected parse
//! tree in this notation. It makes operator precedence and associativity
//! explicit, which a flat SQL rendering would hide.

use crate::ast::{
    Aggregate, AggregateKind, ColumnRef, CompareOp, Condition, Delete, Insert, JoinClause,
    JoinKind, Literal, Operand, OrderItem, Projection, RowCount, Select, SelectItem, Statement,
    TableRef, Update, ValueExpr,
};

/// Renders `statement` as a single-line S-expression.
pub(super) fn statement(statement: &Statement) -> String {
    match statement {
        Statement::Select(select) => self::select(select),
        Statement::Insert(insert) => self::insert(insert),
        Statement::Update(update) => self::update(update),
        Statement::Delete(delete) => self::delete(delete),
        Statement::Begin => "(begin)".to_string(),
        Statement::Commit => "(commit)".to_string(),
        Statement::Rollback => "(rollback)".to_string(),
    }
}

/// Joins the non-empty `parts` with spaces inside one pair of parentheses.
fn list(parts: Vec<String>) -> String {
    let parts: Vec<String> = parts.into_iter().filter(|part| !part.is_empty()).collect();
    format!("({parts})", parts = parts.join(" "))
}

/// Renders a name bare when it is a plain word, double-quoted otherwise.
fn name(name: &str) -> String {
    let plain = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if plain {
        name.to_string()
    } else {
        format!("\"{escaped}\"", escaped = name.replace('"', "\"\""))
    }
}

fn names(head: &str, names: &[String]) -> String {
    let mut parts = vec![head.to_string()];
    parts.extend(names.iter().map(|n| name(n)));
    list(parts)
}

fn select(select: &Select) -> String {
    let mut columns = vec!["columns".to_string()];
    match &select.projection {
        Projection::All => columns.push("*".to_string()),
        Projection::Items(items) => columns.extend(items.iter().map(select_item)),
    }
    let mut parts = vec!["select".to_string()];
    if select.distinct {
        parts.push("distinct".to_string());
    }
    parts.push(list(columns));
    parts.push(list(vec!["from".to_string(), table_ref(&select.from)]));
    parts.extend(select.joins.iter().map(join));
    parts.push(clause("where", select.filter.as_ref()));
    if !select.group_by.is_empty() {
        let mut group_by = vec!["group-by".to_string()];
        group_by.extend(select.group_by.iter().map(column_ref));
        parts.push(list(group_by));
    }
    parts.push(clause("having", select.having.as_ref()));
    if !select.order_by.is_empty() {
        let mut order_by = vec!["order-by".to_string()];
        order_by.extend(select.order_by.iter().map(order_item));
        parts.push(list(order_by));
    }
    parts.push(row_count("limit", select.limit.as_ref()));
    parts.push(row_count("offset", select.offset.as_ref()));
    list(parts)
}

fn select_item(item: &SelectItem) -> String {
    match &item.alias {
        Some(alias) => list(vec!["as".to_string(), operand(&item.operand), name(alias)]),
        None => operand(&item.operand),
    }
}

fn table_ref(table: &TableRef) -> String {
    match &table.alias {
        Some(alias) => list(vec!["as".to_string(), name(&table.name), name(alias)]),
        None => name(&table.name),
    }
}

fn join(join: &JoinClause) -> String {
    let kind = match join.kind {
        JoinKind::Inner => "inner",
        JoinKind::Left => "left",
        JoinKind::Right => "right",
        JoinKind::Full => "full",
    };
    list(vec![
        "join".to_string(),
        kind.to_string(),
        table_ref(&join.table),
        list(vec![
            "on".to_string(),
            column_ref(&join.left),
            column_ref(&join.right),
        ]),
    ])
}

fn order_item(item: &OrderItem) -> String {
    let direction = if item.descending { "desc" } else { "asc" };
    list(vec![operand(&item.operand), direction.to_string()])
}

fn row_count(head: &str, count: Option<&RowCount>) -> String {
    match count {
        Some(RowCount::Value(value)) => list(vec![head.to_string(), value.to_string()]),
        Some(RowCount::Parameter(index)) => list(vec![head.to_string(), format!("?{index}")]),
        None => String::new(),
    }
}

/// Renders an optional `WHERE` or `HAVING` clause; empty when absent.
fn clause(head: &str, condition: Option<&Condition>) -> String {
    condition
        .map(|condition| list(vec![head.to_string(), self::condition(condition)]))
        .unwrap_or_default()
}

fn insert(insert: &Insert) -> String {
    let mut values = vec!["values".to_string()];
    values.extend(insert.values.iter().map(value));
    list(vec![
        "insert".to_string(),
        name(&insert.table),
        names("columns", &insert.columns),
        list(values),
    ])
}

fn update(update: &Update) -> String {
    let mut set = vec!["set".to_string()];
    set.extend(
        update
            .assignments
            .iter()
            .map(|assignment| list(vec![name(&assignment.column), value(&assignment.value)])),
    );
    list(vec![
        "update".to_string(),
        name(&update.table),
        list(set),
        clause("where", Some(&update.filter)),
    ])
}

fn delete(delete: &Delete) -> String {
    let behavior = if delete.cascade {
        "cascade"
    } else {
        "restrict"
    };
    list(vec![
        "delete".to_string(),
        name(&delete.table),
        clause("where", Some(&delete.filter)),
        behavior.to_string(),
    ])
}

fn condition(condition: &Condition) -> String {
    match condition {
        Condition::And(left, right) => list(vec![
            "and".to_string(),
            self::condition(left),
            self::condition(right),
        ]),
        Condition::Or(left, right) => list(vec![
            "or".to_string(),
            self::condition(left),
            self::condition(right),
        ]),
        Condition::Not(inner) => list(vec!["not".to_string(), self::condition(inner)]),
        Condition::Compare { operand, op, value } => {
            let op = match op {
                CompareOp::Eq => "=",
                CompareOp::NotEq => "!=",
                CompareOp::Lt => "<",
                CompareOp::LtEq => "<=",
                CompareOp::Gt => ">",
                CompareOp::GtEq => ">=",
            };
            list(vec![
                op.to_string(),
                self::operand(operand),
                self::value(value),
            ])
        }
        Condition::In {
            operand,
            values,
            negated,
        } => {
            let head = if *negated { "not-in" } else { "in" };
            let mut parts = vec![head.to_string(), self::operand(operand)];
            parts.extend(values.iter().map(self::value));
            list(parts)
        }
        Condition::Like {
            operand,
            pattern,
            negated,
        } => {
            let head = if *negated { "not-like" } else { "like" };
            list(vec![
                head.to_string(),
                self::operand(operand),
                self::value(pattern),
            ])
        }
        Condition::IsNull { operand, negated } => {
            let head = if *negated { "is-not-null" } else { "is-null" };
            list(vec![head.to_string(), self::operand(operand)])
        }
    }
}

fn operand(operand: &Operand) -> String {
    match operand {
        Operand::Column(column) => column_ref(column),
        Operand::Aggregate(aggregate) => self::aggregate(aggregate),
    }
}

fn aggregate(aggregate: &Aggregate) -> String {
    let function = match aggregate.function {
        AggregateKind::Count => "count",
        AggregateKind::Sum => "sum",
        AggregateKind::Avg => "avg",
        AggregateKind::Min => "min",
        AggregateKind::Max => "max",
    };
    let argument = aggregate
        .column
        .as_ref()
        .map_or_else(|| "*".to_string(), column_ref);
    list(vec![function.to_string(), argument])
}

fn column_ref(column: &ColumnRef) -> String {
    match &column.table {
        Some(table) => format!(
            "{table}.{column}",
            table = name(table),
            column = name(&column.column)
        ),
        None => name(&column.column),
    }
}

fn value(value: &ValueExpr) -> String {
    match value {
        ValueExpr::Parameter(index) => format!("?{index}"),
        ValueExpr::Literal(Literal::Integer(value)) => value.to_string(),
        ValueExpr::Literal(Literal::Float(value)) => value.clone(),
        ValueExpr::Literal(Literal::String(value)) => {
            format!("'{escaped}'", escaped = value.replace('\'', "''"))
        }
        ValueExpr::Literal(Literal::Boolean(true)) => "true".to_string(),
        ValueExpr::Literal(Literal::Boolean(false)) => "false".to_string(),
        ValueExpr::Literal(Literal::Null) => "null".to_string(),
    }
}
