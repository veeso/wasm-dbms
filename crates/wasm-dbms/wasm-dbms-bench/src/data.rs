use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

/// Deterministic test-data generator for reproducible benchmarks.
pub struct DataGenerator {
    rng: StdRng,
}

/// Raw record data that can be inserted into any engine.
pub struct UserData {
    pub id: u32,
    pub name: String,
    pub email: String,
    pub age: u32,
}

/// Raw post record data that can be inserted into any engine.
pub struct PostData {
    pub id: u32,
    pub title: String,
    pub body: String,
    pub user_id: u32,
}

impl Default for DataGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl DataGenerator {
    /// Creates a new generator with a fixed seed for reproducibility.
    pub fn new() -> Self {
        Self {
            rng: StdRng::seed_from_u64(42),
        }
    }

    /// Generates `count` user records with sequential IDs starting from 1.
    pub fn users(&mut self, count: u32) -> Vec<UserData> {
        (1..=count)
            .map(|id| {
                let age = self.rng.random_range(18..80);
                UserData {
                    id,
                    name: format!("user_{id}"),
                    email: format!("user_{id}@example.com"),
                    age,
                }
            })
            .collect()
    }

    /// Generates `count` post records assigned round-robin to `num_users` users.
    ///
    /// User IDs are assumed to start from 1.
    pub fn posts(&mut self, count: u32, num_users: u32) -> Vec<PostData> {
        (1..=count)
            .map(|id| {
                let user_id = ((id - 1) % num_users) + 1;
                PostData {
                    id,
                    title: format!("Post title {id}"),
                    body: format!("Body of post {id} by user {user_id}"),
                    user_id,
                }
            })
            .collect()
    }
}
