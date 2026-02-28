use candid::CandidType;
use ic_dbms_api::prelude::{Principal, Text, Uint32};
use ic_dbms_canister::prelude::{
    DbmsCanister, EmailValidator, LowerCaseSanitizer, MaxStrlenValidator, Table, TrimSanitizer,
};
use serde::Deserialize;

#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,
    #[sanitizer(TrimSanitizer)]
    #[validate(MaxStrlenValidator(20))]
    pub name: Text,
    #[sanitizer(LowerCaseSanitizer)]
    #[validate(EmailValidator)]
    pub email: Text,
}

#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
#[table = "posts"]
pub struct Post {
    #[primary_key]
    pub id: Uint32,
    pub title: Text,
    pub content: Text,
    #[foreign_key(entity = "User", table = "users", column = "id")]
    pub user: Uint32,
}

#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
#[table = "projects"]
pub struct Project {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,
    #[custom_type]
    pub owner: Principal,
}

#[derive(DbmsCanister)]
#[tables(User = "users", Post = "posts", Project = "projects")]
pub struct IcDbmsCanisterGenerator;

ic_cdk::export_candid!();
