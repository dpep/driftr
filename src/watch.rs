/// Decides when a changing working tree has settled enough to snapshot.
///
/// Fed the tree observed on each poll: a tree seen twice in a row has settled;
/// `max_ticks` of continuous churn forces a snapshot anyway.
pub struct Settle {
    last: Option<String>,
    unsettled: u32,
    max_ticks: u32,
}

impl Settle {
    pub fn new(max_ticks: u32) -> Settle {
        Settle {
            last: None,
            unsettled: 0,
            max_ticks: max_ticks.max(1),
        }
    }

    /// True means snapshot now. Snapshots are idempotent, so repeated trues while idle are harmless.
    pub fn observe(&mut self, tree: String) -> bool {
        let settled = self.last.as_ref() == Some(&tree);
        self.last = Some(tree);
        if settled {
            self.unsettled = 0;
            return true;
        }
        self.unsettled += 1;
        if self.unsettled >= self.max_ticks {
            self.unsettled = 0;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_once_a_tree_repeats() {
        let mut s = Settle::new(10);
        assert!(!s.observe("a".into()));
        assert!(s.observe("a".into()));
    }

    #[test]
    fn waits_while_tree_keeps_changing() {
        let mut s = Settle::new(10);
        assert!(!s.observe("a".into()));
        assert!(!s.observe("b".into()));
        assert!(!s.observe("c".into()));
        assert!(s.observe("c".into()));
    }

    #[test]
    fn forces_a_snapshot_under_continuous_churn() {
        let mut s = Settle::new(3);
        assert!(!s.observe("a".into()));
        assert!(!s.observe("b".into()));
        assert!(s.observe("c".into()));
        assert!(!s.observe("d".into()));
    }
}
