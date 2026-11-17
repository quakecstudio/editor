// SPDX-License-Identifier: GPL-3.0-or-later

//! One shard of a list, `k/n`: every n-th item from the k-th, k counting from 0. `cargo xtask
//! compiler-diff --functions --shard` splits a codebase's functions so (DIFF-15.10).

/// One shard of a list, `k/n`, with k below n.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Shard {
    index: u64,
    count: u64,
}

impl Shard {
    /// `k/n`, with k below n.
    pub(crate) fn new(index: u64, count: u64) -> Result<Shard, String> {
        if count == 0 {
            return Err("a shard count must be at least 1".to_owned());
        }
        if index >= count {
            return Err(format!(
                "shard {index}/{count}: the shard number must be less than the count"
            ));
        }
        Ok(Shard { index, count })
    }

    /// `"k/n"`.
    pub(crate) fn parse(text: &str) -> Result<Shard, String> {
        let Some((index, count)) = text.split_once('/') else {
            return Err(format!("a shard is written k/n, not {text:?}"));
        };
        let number = |part: &str| {
            part.parse::<u64>()
                .map_err(|error| format!("shard {text:?}: {part:?} is not a count: {error}"))
        };
        Shard::new(number(index)?, number(count)?)
    }

    /// Whether the `item`th of a list falls to this shard: every n-th, from the k-th.
    pub(crate) fn holds(self, item: usize) -> bool {
        u64::try_from(item).is_ok_and(|item| item.checked_rem(self.count) == Some(self.index))
    }
}

impl std::fmt::Display for Shard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.index, self.count)
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "test code: a failed unwrap or index is a failed test"
    )]

    use super::Shard;

    #[test]
    fn a_shard_is_k_of_n_with_k_below_n() {
        assert_eq!(Shard::parse("1/4").unwrap().to_string(), "1/4");
        assert_eq!(Shard::parse("9/10").unwrap(), Shard::new(9, 10).unwrap());
        for bad in [
            "", "1", "1/", "/4", "a/4", "1/b", "-1/4", "1/2/3", "4/4", "5/4", "0/0", "1/0",
        ] {
            let error = Shard::parse(bad).unwrap_err();
            assert!(error.contains("shard"), "{bad:?}: {error}");
        }
        assert!(
            Shard::new(4, 4)
                .unwrap_err()
                .contains("less than the count")
        );
        assert!(Shard::new(0, 0).unwrap_err().contains("at least 1"));
    }

    #[test]
    fn each_item_falls_to_one_shard() {
        let shards: Vec<Shard> = (0..3).map(|k| Shard::new(k, 3).unwrap()).collect();
        for item in 0..10_usize {
            let holding = shards.iter().filter(|shard| shard.holds(item)).count();
            assert_eq!(holding, 1, "item {item}");
            assert!(shards[item % 3].holds(item));
        }
        assert!(Shard::new(0, 1).unwrap().holds(usize::MAX));
    }
}
