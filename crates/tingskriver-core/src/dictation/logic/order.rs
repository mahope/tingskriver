//! Insertion order of dictations.
//!
//! Each dictation takes a ticket when the shortcut is released. A fast engine
//! can finish a later dictation first, so insertion waits until every earlier
//! ticket is done (inserted, failed or empty).

use std::collections::BTreeSet;

#[derive(Clone, Debug, Default)]
pub struct Order {
    /// Lowest ticket not finished yet; it may insert now.
    next: u64,
    /// Finished tickets above `next`.
    done: BTreeSet<u64>,
}

impl Order {
    pub fn next(&self) -> u64 {
        self.next
    }

    /// Mark `ticket` finished. Returns the new lowest unfinished ticket.
    pub fn finish(&mut self, ticket: u64) -> u64 {
        if ticket >= self.next {
            self.done.insert(ticket);
        }
        while self.done.remove(&self.next) {
            self.next += 1;
        }
        self.next
    }

    /// May `ticket` insert now?
    pub fn may_insert(&self, ticket: u64) -> bool {
        ticket <= self.next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_order_finishes_advance_one_by_one() {
        let mut order = Order::default();
        assert!(order.may_insert(0));
        assert!(!order.may_insert(1));
        assert_eq!(order.finish(0), 1);
        assert!(order.may_insert(1));
        assert_eq!(order.finish(1), 2);
    }

    #[test]
    fn a_later_dictation_waits_for_an_earlier_one() {
        let mut order = Order::default();
        // Ticket 1 (no speech) finishes before ticket 0 has inserted.
        assert_eq!(order.finish(1), 0);
        assert!(!order.may_insert(2));
        assert_eq!(order.finish(0), 2);
        assert!(order.may_insert(2));
    }

    #[test]
    fn finishing_twice_or_late_is_harmless() {
        let mut order = Order::default();
        order.finish(0);
        assert_eq!(order.finish(0), 1);
        order.finish(3);
        order.finish(2);
        assert_eq!(order.next(), 1);
        assert_eq!(order.finish(1), 4);
    }
}
