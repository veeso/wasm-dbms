use ic_agent::Agent;
use ic_dbms_client::prelude::{Client, IcDbmsPocketIcClient};

use crate::{PocketIcTestEnv, TestEnv};

pub async fn init_new_agent(ctx: &PocketIcTestEnv, add_to_acl: bool) -> Agent {
    let endpoint = ctx.endpoint().expect("context must be in live mode");

    let agent = Agent::builder()
        .with_url(endpoint)
        .build()
        .expect("Failed to create agent");

    agent
        .fetch_root_key()
        .await
        .expect("Failed to fetch root key");

    // add agent to ACL if required
    if add_to_acl {
        let canister_client = IcDbmsPocketIcClient::new(ctx.dbms_canister(), ctx.admin(), &ctx.pic);
        let agent_principal = agent
            .get_principal()
            .expect("failed to get agent's principal");
        canister_client
            .acl_add_principal(agent_principal)
            .await
            .expect("failed to call canister")
            .expect("failed to add principal to ACL");
    }

    agent
}
