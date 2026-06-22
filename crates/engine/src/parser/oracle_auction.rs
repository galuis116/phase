//! CR 119.3 + CR 101.4 + CR 608.2: Open-bid life auction parser — the
//! "bid life" family (Illicit Auction, Pain's Reward, Mages' Contest).
//!
//! Recognizes the shared bid spine:
//!
//! ```text
//! <opener> ... You start the bidding with a bid of <N|any number>.
//! In turn order, each player may top the high bid. The bidding ends if the
//! high bid stands. The high bidder loses life equal to the high bid
//! [and <WINNER_EFFECT>]. [If you win the bidding, <WINNER_EFFECT>.]
//! ```
//!
//! Three openers, each fixing the target / opening bid / voter scope:
//!   * "Each player may bid life for control of target creature." (Illicit
//!     Auction) → target = a creature, opening 0, winner gains control.
//!   * "Each player may bid life." (Pain's Reward) → no target, player-chosen
//!     opening, winner draws four cards.
//!   * "You and target spell's controller bid life." (Mages' Contest) → target
//!     = a stack spell, opening 1, winner loses life; counter only if the
//!     caster wins ("If you win the bidding, counter that spell").
//!
//! The WINNER_EFFECT tail is parsed via the production effect parser
//! (`parse_effect_chain_with_context`) into a `Box<AbilityDefinition>`, so
//! "gains control of the creature" / "draws four cards" / "counter that spell"
//! lower through the same paths every other card uses.
//!
//! Architectural rules (oracle-parser skill): nom combinators for ALL dispatch;
//! the detector is pure (returns `None` on any non-match so the caller falls
//! back to the standard chain parser).

use nom::branch::alt;
use nom::bytes::complete::{tag, tag_no_case};
use nom::combinator::value;
use nom::Parser;

use crate::parser::oracle_nom::error::OracleError;
use crate::parser::oracle_nom::primitives::{parse_number, scan_preceded, scan_split_at_phrase};
use crate::types::ability::{
    AbilityDefinition, AbilityKind, AuctionOpening, Effect, TargetFilter, TypedFilter, VoterScope,
};

use super::oracle_effect::parse_effect_chain_with_context;
use super::oracle_ir::context::ParseContext;

/// Detect and parse the entire open-bid life-auction block. Returns a single
/// `AbilityDefinition` whose `effect` is `Effect::AuctionBid`, or `None` if the
/// input is not in the bid-life shape.
pub(crate) fn parse_auction_block(text: &str, kind: AbilityKind) -> Option<AbilityDefinition> {
    // Phase 1: opener — determines target, opening bid, and the location of the
    // winner-effect tail.
    let (target, opening_bid, voter_scope, winner_text) = parse_opener_and_winner(text)?;

    // Phase 2: lower the winner-effect tail via the production effect parser.
    let mut ctx = ParseContext::default();
    let winner_def = parse_effect_chain_with_context(winner_text, kind, &mut ctx);
    // Reject if the winner effect failed to parse into something concrete — a
    // bare Unimplemented tail means we should not synthesize an auction that
    // silently does nothing on settlement.
    if matches!(*winner_def.effect, Effect::Unimplemented { .. }) {
        return None;
    }

    Some(AbilityDefinition::new(
        kind,
        Effect::AuctionBid {
            opening_bid,
            voter_scope,
            winner_effect: Box::new(winner_def),
            target,
        },
    ))
}

/// Recognize the opener + bid spine and return
/// `(target, opening_bid, voter_scope, winner_effect_text)`.
///
/// The shared spine ("In turn order, each player may top the high bid. The
/// bidding ends if the high bid stands. The high bidder loses life equal to the
/// high bid") must be present for any branch to match.
fn parse_opener_and_winner(text: &str) -> Option<(TargetFilter, AuctionOpening, VoterScope, &str)> {
    // The spine that all three cards share, anchoring the winner-effect tail.
    // `scan_preceded` returns `(before, _, tail)` where `tail` is the remainder
    // AFTER the matched loss clause — no manual prefix stripping. Everything
    // after is either " and <WINNER>." (Illicit / Pain's) or ". If you win the
    // bidding, <WINNER>." (Mages').
    let (before_loses, _, tail) = scan_preceded(text, |i| {
        tag_no_case::<_, _, OracleError<'_>>("the high bidder loses life equal to the high bid")
            .parse(i)
    })?;

    // Require the canonical mid-spine in the text preceding the loss clause so
    // we never match a non-auction sentence that merely contains "loses life".
    scan_split_at_phrase(before_loses, |i| {
        tag_no_case::<_, _, OracleError<'_>>("in turn order, each player may top the high bid")
            .parse(i)
    })?;

    // Extract the winner-effect text from the tail.
    let winner_text = parse_winner_tail(tail.trim_start());

    // Phase: opener — fixes target / opening / voter scope.
    let (target, opening_bid, voter_scope) = parse_opener(before_loses)?;

    winner_text.map(|w| (target, opening_bid, voter_scope, w))
}

/// Extract the winner-effect sentence from the post-loss tail.
///
///   * " and gains control of the creature. (This effect ...)" (Illicit) /
///     " and draws four cards." (Pain's) → after the leading "and ".
///   * ". If you win the bidding, counter that spell." (Mages') → after the
///     "if you win the bidding," clause.
fn parse_winner_tail(tail: &str) -> Option<&str> {
    // Mages' Contest: ". If you win the bidding, <WINNER>." `scan_preceded`
    // yields the remainder after the gate clause.
    if let Some((_, _, after)) = scan_preceded(tail, |i| {
        tag_no_case::<_, _, OracleError<'_>>("if you win the bidding,").parse(i)
    }) {
        return Some(strip_winner_sentence(after.trim_start()));
    }

    // Illicit / Pain's: " and <WINNER>." — consume the leading "and ".
    let (_, _, after_and) = scan_preceded(tail, |i| {
        tag_no_case::<_, _, OracleError<'_>>("and ").parse(i)
    })?;
    Some(strip_winner_sentence(after_and.trim_start()))
}

/// Trim the winner sentence at the first reminder-text paren / sentence
/// terminator so trailing reminders ("(This effect lasts indefinitely.)") and
/// the period don't reach the effect parser. Uses `take_until`-style nom scans,
/// not string-method dispatch.
fn strip_winner_sentence(s: &str) -> &str {
    // Cut at the first " (" (reminder text) if present.
    let s = match scan_split_at_phrase(s, |i| tag::<_, _, OracleError<'_>>("(").parse(i)) {
        Some((before, _)) if !before.is_empty() => before,
        _ => s,
    };
    // Cut at the first sentence-ending period.
    let s = match scan_split_at_phrase(s, |i| tag::<_, _, OracleError<'_>>(".").parse(i)) {
        Some((before, _)) if !before.is_empty() => before,
        _ => s,
    };
    s.trim()
}

/// Parse the opener clause, returning `(target, opening_bid, voter_scope)`. The
/// three openers compose into a single `alt`, each mapping its leading tag to
/// the `(target, voter_scope)` it fixes; the opening-bid amount is parsed from
/// the shared "You start the bidding with a bid of ..." clause afterward.
fn parse_opener(text: &str) -> Option<(TargetFilter, AuctionOpening, VoterScope)> {
    let trimmed = text.trim_start();

    // Order matters: "each player may bid life for control of target creature"
    // is a strict prefix-superset of "each player may bid life", so the longer
    // Illicit opener must be tried before the Pain's Reward opener.
    let (_, (target, voter_scope)) = alt((
        value(
            (TargetFilter::StackSpell, VoterScope::AllPlayers),
            tag_no_case::<_, _, OracleError<'_>>("you and target spell's controller bid life"),
        ),
        value(
            (
                TargetFilter::Typed(TypedFilter::creature()),
                VoterScope::AllPlayers,
            ),
            tag_no_case::<_, _, OracleError<'_>>(
                "each player may bid life for control of target creature",
            ),
        ),
        value(
            (TargetFilter::None, VoterScope::AllPlayers),
            tag_no_case::<_, _, OracleError<'_>>("each player may bid life"),
        ),
    ))
    .parse(trimmed)
    .ok()?;

    let opening = parse_opening_amount(trimmed)?;
    Some((target, opening, voter_scope))
}

/// CR 119.3: Parse the opening-bid clause "You start the bidding with a bid of
/// <N | any number>." Returns `AuctionOpening::Fixed(N)` or
/// `AuctionOpening::PlayerChosen`.
fn parse_opening_amount(text: &str) -> Option<AuctionOpening> {
    let (_, _, after) = scan_preceded(text, |i| {
        tag_no_case::<_, _, OracleError<'_>>("you start the bidding with a bid of").parse(i)
    })?;
    let after = after.trim_start();

    // "any number" → player-chosen; otherwise a fixed numeric opening. Compose
    // as a single alt so the discriminant is one combinator, not chained tries.
    alt((
        value(
            AuctionOpening::PlayerChosen,
            tag_no_case::<_, _, OracleError<'_>>("any number"),
        ),
        nom::combinator::map(parse_number, AuctionOpening::Fixed),
    ))
    .parse(after)
    .ok()
    .map(|(_, opening)| opening)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> AbilityDefinition {
        parse_auction_block(text, AbilityKind::Spell)
            .unwrap_or_else(|| panic!("auction block must parse: {text}"))
    }

    #[test]
    fn illicit_auction_parses_to_gain_control_creature_target_opening_zero() {
        let text = "Each player may bid life for control of target creature. \
                    You start the bidding with a bid of 0. In turn order, each \
                    player may top the high bid. The bidding ends if the high \
                    bid stands. The high bidder loses life equal to the high bid \
                    and gains control of the creature. (This effect lasts \
                    indefinitely.)";
        let def = parse(text);
        let Effect::AuctionBid {
            opening_bid,
            winner_effect,
            target,
            ..
        } = &*def.effect
        else {
            panic!("expected AuctionBid, got {:?}", def.effect);
        };
        assert_eq!(*opening_bid, AuctionOpening::Fixed(0));
        assert!(matches!(target, TargetFilter::Typed(_)), "creature target");
        assert!(matches!(*winner_effect.effect, Effect::GainControl { .. }));
    }

    #[test]
    fn pains_reward_parses_to_draw_four_player_chosen_no_target() {
        let text = "Each player may bid life. You start the bidding with a bid \
                    of any number. In turn order, each player may top the high \
                    bid. The bidding ends if the high bid stands. The high \
                    bidder loses life equal to the high bid and draws four cards.";
        let def = parse(text);
        let Effect::AuctionBid {
            opening_bid,
            winner_effect,
            target,
            ..
        } = &*def.effect
        else {
            panic!("expected AuctionBid, got {:?}", def.effect);
        };
        assert_eq!(*opening_bid, AuctionOpening::PlayerChosen);
        assert_eq!(*target, TargetFilter::None);
        assert!(matches!(*winner_effect.effect, Effect::Draw { .. }));
    }

    #[test]
    fn mages_contest_parses_to_counter_stack_spell_opening_one() {
        let text = "You and target spell's controller bid life. You start the \
                    bidding with a bid of 1. In turn order, each player may top \
                    the high bid. The bidding ends if the high bid stands. The \
                    high bidder loses life equal to the high bid. If you win the \
                    bidding, counter that spell.";
        let def = parse(text);
        let Effect::AuctionBid {
            opening_bid,
            winner_effect,
            target,
            ..
        } = &*def.effect
        else {
            panic!("expected AuctionBid, got {:?}", def.effect);
        };
        assert_eq!(*opening_bid, AuctionOpening::Fixed(1));
        assert_eq!(*target, TargetFilter::StackSpell);
        assert!(matches!(*winner_effect.effect, Effect::Counter { .. }));
    }

    #[test]
    fn non_auction_text_returns_none() {
        assert!(parse_auction_block("Draw a card.", AbilityKind::Spell).is_none());
        assert!(
            parse_auction_block("Target creature loses all abilities.", AbilityKind::Spell)
                .is_none()
        );
    }
}
