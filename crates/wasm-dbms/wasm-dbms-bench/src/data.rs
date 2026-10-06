use std::num::NonZeroU32;

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

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
    /// Every post references an existing user, so at least one user is
    /// required; [`NonZeroU32`] makes an empty user population
    /// unrepresentable. User IDs are assumed to be `1..=num_users`, as
    /// produced by [`DataGenerator::users`].
    pub fn posts(&mut self, count: u32, num_users: NonZeroU32) -> Vec<PostData> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_posts_with_a_single_user_assign_every_post_to_it() {
        let posts = DataGenerator::new().posts(5, NonZeroU32::MIN);

        assert_eq!(posts.len(), 5);
        assert!(posts.iter().all(|post| post.user_id == 1));
        assert_eq!(
            posts.iter().map(|post| post.id).collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5]
        );
    }

    #[test]
    fn test_posts_are_assigned_round_robin_to_existing_users() {
        let num_users = NonZeroU32::new(3).expect("three is nonzero");
        let posts = DataGenerator::new().posts(7, num_users);

        assert_eq!(
            posts.iter().map(|post| post.user_id).collect::<Vec<_>>(),
            vec![1, 2, 3, 1, 2, 3, 1]
        );
        assert!(
            posts
                .iter()
                .all(|post| (1..=num_users.get()).contains(&post.user_id))
        );
    }
}
