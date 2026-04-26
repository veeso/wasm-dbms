//! Prelude module for ic-dbms-client

pub use ic_dbms_api::prelude::{
    AggregateFunction, AggregatedRow, AggregatedValue, Blob, Boolean, CandidDataTypeKind,
    ColumnDef, DataTypeKind, Date, DateTime, Decimal, DeleteBehavior, Filter, ForeignKeyDef,
    InsertRecord, Int8, Int16, Int32, Int64, Json, JsonCmp, JsonFilter, Nullable, OrderDirection,
    Principal, Query, QueryBuilder, Select, TableColumns, TableError, TableRecord, Text, Uint8,
    Uint16, Uint32, Uint64, UpdateRecord, Uuid, Value, ValuesSource,
};

#[cfg(feature = "ic-agent")]
#[cfg_attr(docsrs, doc(cfg(feature = "ic-agent")))]
pub use crate::client::IcDbmsAgentClient;
#[cfg(feature = "pocket-ic")]
#[cfg_attr(docsrs, doc(cfg(feature = "pocket-ic")))]
pub use crate::client::IcDbmsPocketIcClient;
pub use crate::client::{Client, IcDbmsCanisterClient};
#[cfg(feature = "ic-agent")]
#[cfg_attr(docsrs, doc(cfg(feature = "ic-agent")))]
pub use crate::errors::IcAgentError;
#[cfg(feature = "pocket-ic")]
#[cfg_attr(docsrs, doc(cfg(feature = "pocket-ic")))]
pub use crate::errors::PocketIcError;
pub use crate::errors::{IcDbmCanisterClientError, IcDbmsCanisterClientResult};
