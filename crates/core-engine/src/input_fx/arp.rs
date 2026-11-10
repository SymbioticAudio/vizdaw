//! The arpeggiator stage (plan 5g-1 Task 3; Rulings 9 and 10). Until Task 3
//! it is an empty stage: the chain passes its input through unchanged.

use core_model::input_fx::Arp;

/// An arpeggiator's running state.
pub(crate) struct ArpState;

impl ArpState {
    /// A fresh arpeggiator for `_arp`. Allocates; control thread only.
    pub(crate) fn new(_: &Arp) -> Box<ArpState> {
        Box::new(ArpState)
    }
}
