use super::{
    CNW_FRAGMENT_HEADER_BITS, CREATURE_OBJECT_TYPE, bits, claim_payload_if_verified,
    live_object_payload_from_parts, looks_like_bounded_cnw_fragment_storage_span,
    verified_work_remaining_record_legal_end,
};

const TERMINAL_FRAGMENT_CLAIM_SLOTS: usize = 5;

/// A typed owner candidate for terminal CNW fragment storage.
///
/// Registration is evidence, not authority. The final evaluator requires the
/// exact final cursor for every registered owner and repeats the historical
/// truncated-packet validator where that family already had one. Unresolved
/// door/placeable tail9 has no claim variant or registration path until its
/// server writer is proven; successfully typed tail9 can still finish through
/// the existing generic family claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TerminalFragmentClaim {
    Family { bit_cursor: usize },
    CreatureUpdate { bit_cursor: usize },
    LiveGuiItem { bit_cursor: usize },
    PromotedStorage { bit_cursor: usize },
    WorkRemainingStorage { offset: usize, legal_end: usize },
}

impl TerminalFragmentClaim {
    fn slot(self) -> usize {
        match self {
            Self::Family { .. } => 0,
            Self::CreatureUpdate { .. } => 1,
            Self::LiveGuiItem { .. } => 2,
            Self::PromotedStorage { .. } => 3,
            Self::WorkRemainingStorage { .. } => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct TerminalFragmentClaimSet {
    claims: [Option<TerminalFragmentClaim>; TERMINAL_FRAGMENT_CLAIM_SLOTS],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TerminalFragmentDisposition {
    NoResidual,
    Trim {
        owner: TerminalFragmentClaim,
        bit_cursor: usize,
    },
    RejectUnowned {
        bit_cursor: usize,
    },
}

#[derive(Debug, Clone, Copy)]
pub(super) struct TerminalFragmentEvaluation {
    pub disposition: TerminalFragmentDisposition,
    pub work_remaining_storage: Option<(usize, usize)>,
    pub work_remaining_non_empty_storage_allowed: bool,
}

impl TerminalFragmentClaimSet {
    pub fn register(&mut self, claim: TerminalFragmentClaim) {
        self.claims[claim.slot()] = Some(claim);
    }

    pub fn clear_work_remaining_storage(&mut self) {
        self.claims[TerminalFragmentClaim::WorkRemainingStorage {
            offset: 0,
            legal_end: 0,
        }
        .slot()] = None;
    }

    pub fn replace_work_remaining_storage(&mut self, record: Option<(usize, usize)>) {
        self.clear_work_remaining_storage();
        if let Some((offset, legal_end)) = record {
            self.register(TerminalFragmentClaim::WorkRemainingStorage { offset, legal_end });
        }
    }

    pub fn replace_promoted_storage(
        &mut self,
        start_bit_cursor: usize,
        end_bit_cursor: usize,
        bits_promoted: usize,
    ) {
        let slot = TerminalFragmentClaim::PromotedStorage { bit_cursor: 0 }.slot();
        self.claims[slot] = end_bit_cursor
            .checked_sub(start_bit_cursor)
            .filter(|consumed_bits| bits_promoted > *consumed_bits)
            .map(|_| TerminalFragmentClaim::PromotedStorage {
                bit_cursor: end_bit_cursor,
            });
    }

    fn registered(&self, slot: usize) -> Option<TerminalFragmentClaim> {
        self.claims.get(slot).copied().flatten()
    }

    fn work_remaining_storage(&self) -> Option<(usize, usize)> {
        match self.registered(
            TerminalFragmentClaim::WorkRemainingStorage {
                offset: 0,
                legal_end: 0,
            }
            .slot(),
        ) {
            Some(TerminalFragmentClaim::WorkRemainingStorage { offset, legal_end }) => {
                Some((offset, legal_end))
            }
            _ => None,
        }
    }

    pub fn evaluate(
        &self,
        live_bytes: &[u8],
        fragment_bits: &[bool],
        bit_cursor: usize,
        changed: bool,
        trim_gate_open: bool,
    ) -> TerminalFragmentEvaluation {
        let work_remaining_storage = self.work_remaining_storage();
        let work_remaining_non_empty_storage_allowed = changed
            && work_remaining_has_bounded_storage_suffix_within_cursor(
                live_bytes,
                work_remaining_storage,
                bit_cursor,
            );

        if bit_cursor >= fragment_bits.len() {
            return TerminalFragmentEvaluation {
                disposition: TerminalFragmentDisposition::NoResidual,
                work_remaining_storage,
                work_remaining_non_empty_storage_allowed,
            };
        }
        if !trim_gate_open {
            return TerminalFragmentEvaluation {
                disposition: TerminalFragmentDisposition::RejectUnowned { bit_cursor },
                work_remaining_storage,
                work_remaining_non_empty_storage_allowed,
            };
        }

        if let Some(
            owner @ TerminalFragmentClaim::Family {
                bit_cursor: registered,
            },
        ) = self.registered(TerminalFragmentClaim::Family { bit_cursor: 0 }.slot())
        {
            if registered == bit_cursor
                && fragment_trim_exact_claim_allowed(live_bytes, fragment_bits, bit_cursor)
            {
                return self.trim_evaluation(
                    owner,
                    bit_cursor,
                    work_remaining_storage,
                    work_remaining_non_empty_storage_allowed,
                );
            }
        }

        if let Some(
            owner @ TerminalFragmentClaim::CreatureUpdate {
                bit_cursor: registered,
            },
        ) = self.registered(TerminalFragmentClaim::CreatureUpdate { bit_cursor: 0 }.slot())
        {
            if registered == bit_cursor {
                return self.trim_evaluation(
                    owner,
                    bit_cursor,
                    work_remaining_storage,
                    work_remaining_non_empty_storage_allowed,
                );
            }
        }
        if exact_creature_update_trim_allowed(live_bytes, fragment_bits, bit_cursor) {
            return self.trim_evaluation(
                TerminalFragmentClaim::CreatureUpdate { bit_cursor },
                bit_cursor,
                work_remaining_storage,
                work_remaining_non_empty_storage_allowed,
            );
        }

        if let Some(
            owner @ TerminalFragmentClaim::LiveGuiItem {
                bit_cursor: registered,
            },
        ) = self.registered(TerminalFragmentClaim::LiveGuiItem { bit_cursor: 0 }.slot())
        {
            if registered == bit_cursor
                && live_gui_item_trim_exact_claim_allowed(live_bytes, fragment_bits, bit_cursor)
            {
                return self.trim_evaluation(
                    owner,
                    bit_cursor,
                    work_remaining_storage,
                    work_remaining_non_empty_storage_allowed,
                );
            }
        }

        if let Some(owner @ TerminalFragmentClaim::WorkRemainingStorage { .. }) = self.registered(
            TerminalFragmentClaim::WorkRemainingStorage {
                offset: 0,
                legal_end: 0,
            }
            .slot(),
        ) {
            if work_remaining_fragment_bits_trim_allowed(
                live_bytes,
                fragment_bits,
                bit_cursor,
                work_remaining_storage,
                work_remaining_non_empty_storage_allowed,
            ) {
                return self.trim_evaluation(
                    owner,
                    bit_cursor,
                    work_remaining_storage,
                    work_remaining_non_empty_storage_allowed,
                );
            }
        }

        if let Some(
            owner @ TerminalFragmentClaim::PromotedStorage {
                bit_cursor: registered,
            },
        ) = self.registered(TerminalFragmentClaim::PromotedStorage { bit_cursor: 0 }.slot())
        {
            if registered == bit_cursor {
                return self.trim_evaluation(
                    owner,
                    bit_cursor,
                    work_remaining_storage,
                    work_remaining_non_empty_storage_allowed,
                );
            }
        }

        TerminalFragmentEvaluation {
            disposition: TerminalFragmentDisposition::RejectUnowned { bit_cursor },
            work_remaining_storage,
            work_remaining_non_empty_storage_allowed,
        }
    }

    fn trim_evaluation(
        &self,
        owner: TerminalFragmentClaim,
        bit_cursor: usize,
        work_remaining_storage: Option<(usize, usize)>,
        work_remaining_non_empty_storage_allowed: bool,
    ) -> TerminalFragmentEvaluation {
        TerminalFragmentEvaluation {
            disposition: TerminalFragmentDisposition::Trim { owner, bit_cursor },
            work_remaining_storage,
            work_remaining_non_empty_storage_allowed,
        }
    }
}

pub(super) fn fragment_trim_exact_claim_allowed(
    live_bytes: &[u8],
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> bool {
    let mut candidate_bits = fragment_bits.to_vec();
    candidate_bits.truncate(bit_cursor);
    live_object_payload_from_parts(live_bytes, &candidate_bits)
        .and_then(|payload| claim_payload_if_verified(&payload))
        .is_some()
}

fn exact_creature_update_trim_allowed(
    live_bytes: &[u8],
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> bool {
    let mut candidate_bits = fragment_bits.to_vec();
    candidate_bits.truncate(bit_cursor);
    let Some(claim) = live_object_payload_from_parts(live_bytes, &candidate_bits)
        .and_then(|payload| claim_payload_if_verified(&payload))
    else {
        return false;
    };
    claim.mentions.last().is_some_and(|mention| {
        mention.opcode == b'U'
            && mention.object_type == CREATURE_OBJECT_TYPE
            && mention.fragment_bit_end == bit_cursor
            && mention.record_end == live_bytes.len()
    })
}

fn live_gui_item_trim_exact_claim_allowed(
    live_bytes: &[u8],
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> bool {
    let mut candidate_bits = fragment_bits.to_vec();
    candidate_bits.truncate(bit_cursor);
    let Some(claim) = live_object_payload_from_parts(live_bytes, &candidate_bits)
        .and_then(|payload| claim_payload_if_verified(&payload))
    else {
        return false;
    };
    claim.last_live_gui_item_record_end == Some(live_bytes.len())
        && claim.last_live_gui_item_fragment_bit_end == Some(bit_cursor)
}

fn work_remaining_fragment_bits_trim_allowed(
    live_bytes: &[u8],
    fragment_bits: &[bool],
    bit_cursor: usize,
    terminal_record: Option<(usize, usize)>,
    allow_non_empty_storage: bool,
) -> bool {
    let Some((offset, legal_end)) = terminal_record else {
        return false;
    };
    if bit_cursor >= fragment_bits.len()
        || legal_end > live_bytes.len()
        || verified_work_remaining_record_legal_end(live_bytes, offset) != Some(legal_end)
    {
        return false;
    }
    if legal_end >= live_bytes.len()
        || !looks_like_bounded_cnw_fragment_storage_span(&live_bytes[legal_end..])
        || (!allow_non_empty_storage && !fragment_storage_span_is_empty(&live_bytes[legal_end..]))
    {
        return false;
    }
    fragment_trim_exact_claim_allowed(&live_bytes[..legal_end], fragment_bits, bit_cursor)
}

fn work_remaining_has_bounded_storage_suffix_within_cursor(
    live_bytes: &[u8],
    terminal_record: Option<(usize, usize)>,
    bit_cursor: usize,
) -> bool {
    let Some((_offset, legal_end)) = terminal_record else {
        return false;
    };
    if legal_end >= live_bytes.len()
        || !looks_like_bounded_cnw_fragment_storage_span(&live_bytes[legal_end..])
    {
        return false;
    }
    let Some(decoded) =
        bits::decode_msb_valid_bits(&live_bytes[legal_end..], CNW_FRAGMENT_HEADER_BITS)
    else {
        return false;
    };
    let payload_bits = decoded.len().saturating_sub(CNW_FRAGMENT_HEADER_BITS);
    payload_bits != 0 && payload_bits <= bit_cursor
}

pub(super) fn fragment_storage_span_is_empty(span: &[u8]) -> bool {
    bits::decode_msb_valid_bits(span, CNW_FRAGMENT_HEADER_BITS).is_some_and(|decoded| {
        decoded
            .iter()
            .skip(CNW_FRAGMENT_HEADER_BITS)
            .all(|bit| !*bit)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_claim_promoted_storage_requires_the_registered_cursor() {
        let mut claims = TerminalFragmentClaimSet::default();
        claims.register(TerminalFragmentClaim::PromotedStorage { bit_cursor: 4 });
        let bits = [false, false, false, true, false];

        assert_eq!(
            claims.evaluate(&[], &bits, 4, true, true).disposition,
            TerminalFragmentDisposition::Trim {
                owner: TerminalFragmentClaim::PromotedStorage { bit_cursor: 4 },
                bit_cursor: 4,
            }
        );
        assert_eq!(
            claims.evaluate(&[], &bits, 3, true, true).disposition,
            TerminalFragmentDisposition::RejectUnowned { bit_cursor: 3 }
        );
        assert_eq!(
            claims.evaluate(&[], &bits, 4, true, false).disposition,
            TerminalFragmentDisposition::RejectUnowned { bit_cursor: 4 },
            "registered evidence cannot authorize a trim outside the reliable residual gate"
        );
    }

    #[test]
    fn terminal_claim_rejects_residual_without_a_registered_owner() {
        let claims = TerminalFragmentClaimSet::default();
        let bits = [false, false, false, true];

        assert_eq!(
            claims.evaluate(&[], &bits, 3, false, true).disposition,
            TerminalFragmentDisposition::RejectUnowned { bit_cursor: 3 }
        );
        assert_eq!(
            claims
                .evaluate(&[], &bits, bits.len(), false, false)
                .disposition,
            TerminalFragmentDisposition::NoResidual
        );
    }

    #[test]
    fn terminal_claim_promoted_replacement_uses_the_exact_unconsumed_span_rule() {
        let mut claims = TerminalFragmentClaimSet::default();
        claims.replace_promoted_storage(3, 5, 2);
        assert_eq!(
            claims.evaluate(&[], &[false; 6], 5, true, true).disposition,
            TerminalFragmentDisposition::RejectUnowned { bit_cursor: 5 }
        );

        claims.replace_promoted_storage(3, 5, 3);
        assert!(matches!(
            claims.evaluate(&[], &[false; 6], 5, true, true).disposition,
            TerminalFragmentDisposition::Trim {
                owner: TerminalFragmentClaim::PromotedStorage { bit_cursor: 5 },
                bit_cursor: 5,
            }
        ));
    }
}
