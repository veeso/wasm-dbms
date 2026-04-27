use ic_agent::Agent;
use ic_dbms_client::prelude::{Client, IcDbmsPocketIcClient};
use pocket_ic_harness::PocketIcTestEnv;

use crate::{TestCanisterSetup, TestEnvExt, admin};

pub async fn init_new_agent(ctx: &PocketIcTestEnv<TestCanisterSetup>, add_to_acl: bool) -> Agent {
    let endpoint = ctx.endpoint().expect("context must be in live mode");

    let agent = Agent::builder()
        .with_url(endpoint)
        .build()
        .expect("Failed to create agent");

    agent
        .fetch_root_key()
        .await
        .expect("Failed to fetch root key");

    // grant agent full admin perms if required
    if add_to_acl {
        use ic_dbms_api::prelude::TablePerms;
        let canister_client = IcDbmsPocketIcClient::new(ctx.dbms_canister(), admin(), &ctx.pic);
        let agent_principal = agent
            .get_principal()
            .expect("failed to get agent's principal");
        canister_client
            .grant_admin(agent_principal)
            .await
            .expect("failed to call canister")
            .expect("failed to grant admin");
        canister_client
            .grant_manage_acl(agent_principal)
            .await
            .expect("failed to call canister")
            .expect("failed to grant manage_acl");
        canister_client
            .grant_all_tables_perms(agent_principal, TablePerms::all())
            .await
            .expect("failed to call canister")
            .expect("failed to grant table perms");
    }

    agent
}
