use candid::{CandidType, Principal};
use ic_dbms_api::prelude::{Nullable, Query, Table, TableSchema, Text, Uint32, Uint64};
use ic_dbms_client::prelude::{Client as _, IcDbmsCanisterClient};
use serde::Deserialize;

#[derive(Table, CandidType, Clone, Deserialize)]
#[candid]
#[table = "users"]
pub struct User {
    #[primary_key]
    id: Uint64,
    name: Text,
    email: Text,
    age: Nullable<Uint32>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // put your canister principal here
    let principal = Principal::from_text("mxzaz-hqaaa-aaaar-qaada-cai")?;

    let client = IcDbmsCanisterClient::new(principal);

    // insert a new user
    let alice = UserInsertRequest {
        id: 1.into(),
        name: "Alice".into(),
        email: "alice@example.com".into(),
        age: Nullable::Value(30.into()),
    };

    client
        .insert::<User>(User::table_name(), alice, None)
        .await??;

    // select users
    let query = Query::builder().all().build();
    let users = client
        .select::<User>(User::table_name(), query, None)
        .await??;

    for user in users {
        println!(
            "User: id={:?}, name={:?}, email={:?}, age={:?}",
            user.id, user.name, user.email, user.age
        );
    }

    Ok(())
}
