use crate::dbms::query::{Filter, OrderDirection, Query};

/// A builder for constructing database [`Query`]es.
#[derive(Debug, Default, Clone)]
pub struct QueryBuilder {
    query: Query,
}

impl QueryBuilder {
    /// Builds and returns a [`Query`] object based on the current state of the [`QueryBuilder`].
    pub fn build(self) -> Query {
        self.query
    }

    /// Adds a field to select in the query.
    pub fn field(mut self, field: &str) -> Self {
        let field = field.to_string();
        match &mut self.query.columns {
            crate::dbms::query::Select::All => {
                self.query.columns = crate::dbms::query::Select::Columns(vec![field]);
            }
            crate::dbms::query::Select::Columns(cols) if !cols.contains(&field) => {
                cols.push(field);
            }
            _ => {}
        }
        self
    }

    /// Adds multiple fields to select in the query.
    pub fn fields<I>(mut self, fields: I) -> Self
    where
        I: IntoIterator<Item = &'static str>,
    {
        for field in fields {
            self = self.field(field);
        }
        self
    }

    /// Sets the query to select all fields.
    pub fn all(mut self) -> Self {
        self.query.columns = crate::dbms::query::Select::All;
        self
    }

    /// Adds a relation to eagerly load with the main records.
    pub fn with(mut self, table_relation: &str) -> Self {
        let table_relation = table_relation.to_string();
        if !self.query.eager_relations.contains(&table_relation) {
            self.query.eager_relations.push(table_relation);
        }
        self
    }

    /// Adds an ascending order by clause for the specified field.
    pub fn order_by_asc(mut self, field: &str) -> Self {
        self.query
            .order_by
            .push((field.to_string(), OrderDirection::Ascending));
        self
    }

    /// Adds a descending order by clause for the specified field.
    pub fn order_by_desc(mut self, field: &str) -> Self {
        self.query
            .order_by
            .push((field.to_string(), OrderDirection::Descending));
        self
    }

    /// Sets a limit on the number of records to return.
    pub fn limit(mut self, limit: usize) -> Self {
        self.query.limit = Some(limit);
        self
    }

    /// Sets an offset for pagination.
    pub fn offset(mut self, offset: usize) -> Self {
        self.query.offset = Some(offset);
        self
    }

    /// Sets a filter for the query, replacing any existing filter.
    pub fn filter(mut self, filter: Option<Filter>) -> Self {
        self.query.filter = filter;
        self
    }

    /// Adds a filter to the query, combining with existing filters using AND.
    pub fn and_where(mut self, filter: Filter) -> Self {
        self.query.filter = match self.query.filter {
            Some(existing_filter) => Some(existing_filter.and(filter)),
            None => Some(filter),
        };
        self
    }

    /// Adds a filter to the query, combining with existing filters using OR.
    pub fn or_where(mut self, filter: Filter) -> Self {
        self.query.filter = match self.query.filter {
            Some(existing_filter) => Some(existing_filter.or(filter)),
            None => Some(filter),
        };
        self
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::dbms::value::Value;
    use crate::tests::User;

    #[test]
    fn test_default_query_builder() {
        let query_builder = QueryBuilder::default();
        let query = query_builder.build();
        assert!(matches!(query.columns, crate::dbms::query::Select::All));
        assert!(query.eager_relations.is_empty());
        assert!(query.filter.is_none());
        assert!(query.order_by.is_empty());
        assert!(query.limit.is_none());
        assert!(query.offset.is_none());
    }

    #[test]
    fn test_should_add_field_to_query_builder() {
        let query_builder = QueryBuilder::default().field("id").field("name");

        let query = query_builder.build();
        assert_eq!(query.columns::<User>(), vec!["id", "name"]);
    }

    #[test]
    fn test_should_set_fields() {
        let query_builder = QueryBuilder::default().fields(["id", "email"]);

        let query = query_builder.build();
        assert_eq!(query.columns::<User>(), vec!["id", "email"]);
    }

    #[test]
    fn test_should_set_all_fields() {
        let query_builder = QueryBuilder::default().field("id").all();

        let query = query_builder.build();
        assert!(matches!(query.columns, crate::dbms::query::Select::All));
    }

    #[test]
    fn test_should_add_eager_relation() {
        let query_builder = QueryBuilder::default().with("posts");
        let query = query_builder.build();
        assert_eq!(query.eager_relations, vec!["posts"]);
    }

    #[test]
    fn test_should_not_duplicate_eager_relation() {
        let query_builder = QueryBuilder::default().with("posts").with("posts");
        let query = query_builder.build();
        assert_eq!(query.eager_relations, vec!["posts"]);
    }

    #[test]
    fn test_should_add_order_by_clauses() {
        let query_builder = QueryBuilder::default()
            .order_by_asc("name")
            .order_by_desc("created_at");
        let query = query_builder.build();
        assert_eq!(
            query.order_by,
            vec![
                ("name".to_string(), OrderDirection::Ascending),
                ("created_at".to_string(), OrderDirection::Descending)
            ]
        );
    }

    #[test]
    fn test_should_set_limit_and_offset() {
        let query_builder = QueryBuilder::default().limit(10).offset(5);
        let query = query_builder.build();
        assert_eq!(query.limit, Some(10));
        assert_eq!(query.offset, Some(5));
    }

    #[test]
    fn test_should_create_filters() {
        let query = QueryBuilder::default()
            .all()
            .and_where(Filter::eq("id", Value::Uint32(1u32.into())))
            .or_where(Filter::like("name", "John%"))
            .build();

        let filter = query.filter.expect("should have filter");
        if let Filter::Or(left, right) = filter {
            assert!(matches!(*left, Filter::Eq(id, Value::Uint32(_)) if id == "id"));
            assert!(matches!(*right, Filter::Like(name, _) if name == "name"));
        } else {
            panic!("Expected OR filter at the top level");
        }
    }
}
