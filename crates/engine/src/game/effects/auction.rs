//! CR 101.4 + CR 608.2: Open-bid life auction — the "bid life" family (Illicit
//! Auction, Pain's Reward, Mages' Contest).
//!
//! Each eligible player, in turn order starting with the controller (CR 101.4),
//! may top the current high bid ("In turn order, each player may top the high
//! bid"). Bidding ends when the high bid stands — i.e. every OTHER eligible
//! player has passed consecutively (Illicit Auction / Pain's Reward Oracle:
//! "The bidding ends if the high bid stands"). The high bidder then loses life
//! equal to the high bid (CR 119.3) and the `winner_effect` resolves once,
//! bound to the high bidder as controller.
//!
//! The resolver parks on [`WaitingFor::AuctionBid`] for the first actor and the
//! bid loop / settlement lives in `engine_resolution_choices.rs`, mirroring the
//! Vote interactive-queue pattern (`effects/vote.rs`). The winning effect binds
//! its controller to the high bidder via the same `resolved_from_def` shape the
//! vote tally uses for its single-winner outcome (`resolve_threshold_tally`):
//! GainControl reads `original_controller.unwrap_or(controller)` and Draw reads
//! its target filter (defaulting to `controller`), so both resolve FOR the
//! winner when bound this way.

use crate::game::players::apnap_order_from;
use crate::types::ability::{
    AuctionOpening, ControllerRef, Effect, EffectError, EffectKind, ResolvedAbility, TargetRef,
};
use crate::types::events::GameEvent;
use crate::types::game_state::{GameState, WaitingFor};
use crate::types::identifiers::ObjectId;
use crate::types::player::PlayerId;

/// Maximum auction bid representable as signed life loss (`QuantityExpr::Fixed` /
/// `Effect::LoseLife`). Values above this wrap when cast to `i32` and must be
/// rejected at bid submission and settlement.
pub(crate) const MAX_REPRESENTABLE_BID: u32 = i32::MAX as u32;

/// Reject bids that cannot be represented as a signed life-loss amount.
pub(crate) fn bid_amount_in_range(amount: u32) -> Result<(), &'static str> {
    if amount > MAX_REPRESENTABLE_BID {
        Err("bid amount exceeds representable life-loss range")
    } else {
        Ok(())
    }
}

/// CR 101.4: Initiate an open-bid life auction. Resolves the target
/// (creature / stack spell / none), builds the eligible bidder queue, and parks
/// on [`WaitingFor::AuctionBid`] for the first actor.
pub fn resolve(
    state: &mut GameState,
    ability: &ResolvedAbility,
    events: &mut Vec<GameEvent>,
) -> Result<(), EffectError> {
    let Effect::AuctionBid {
        opening_bid,
        voter_scope: _,
        winner_effect,
        target,
    } = &ability.effect
    else {
        return Err(EffectError::InvalidParam(
            "auction::resolve called with non-AuctionBid effect".into(),
        ));
    };

    let controller = ability.controller;

    // CR 115.1: Resolve the declared target object. `TargetFilter::None`
    // (Pain's Reward) yields no object — non-targeting.
    let resolved_target: Option<ObjectId> =
        if matches!(target, crate::types::ability::TargetFilter::None) {
            None
        } else {
            crate::game::targeting::resolved_targets(ability, target, state)
                .into_iter()
                .find_map(|t| match t {
                    TargetRef::Object(id) => Some(id),
                    TargetRef::Player(_) => None,
                })
        };

    // CR 101.4 + CR 800.4g: Build the eligible bidder queue in turn order from
    // the controller, dropping eliminated players.
    //
    // Mages' Contest case: "You and target spell's controller bid life." Only
    // two players bid — the caster and the controller of the targeted stack
    // spell. Detect this by the target being an object on the stack and narrow
    // the queue accordingly (turn order: controller first, then the spell's
    // controller if distinct and non-eliminated).
    let eligible: Vec<PlayerId> =
        match resolved_target.and_then(|id| stack_spell_controller(state, id)) {
            Some(spell_controller) => {
                let mut v = vec![controller];
                if spell_controller != controller
                    && state
                        .players
                        .iter()
                        .any(|p| p.id == spell_controller && !p.is_eliminated)
                {
                    v.push(spell_controller);
                }
                v
            }
            None => apnap_order_from(state, Some(ControllerRef::You), controller)
                .into_iter()
                .collect(),
        };

    if eligible.is_empty() {
        // Defensive: no eligible bidders. Emit EffectResolved so the chain
        // continues rather than parking on an empty auction.
        events.push(GameEvent::EffectResolved {
            kind: EffectKind::AuctionBid,
            source_id: ability.source_id,
        });
        return Ok(());
    }

    let winner_effect = winner_effect.clone();

    match opening_bid {
        AuctionOpening::PlayerChosen => {
            // CR 119.3: "You start the bidding with a bid of any number"
            // (Pain's Reward). The controller's first SubmitBid sets the
            // opening high bid; only then does round-robin topping begin.
            state.waiting_for = WaitingFor::AuctionBid {
                player: controller,
                current_high_bid: 0,
                high_bidder: None,
                eligible,
                remaining_in_round: Vec::new(),
                passes_in_a_row: 0,
                winner_effect,
                target: resolved_target,
                controller,
                source_id: ability.source_id,
            };
        }
        AuctionOpening::Fixed(opening) => {
            // Illicit Auction / Pain's Reward Oracle: "The bidding ends if the high
            // bid stands." Topping then proceeds "in turn order" starting WITH
            // the controller — so the controller acts first and may top their own
            // opening bid. The auction settles when every other eligible player
            // has passed consecutively against the standing high bid.
            let Some((first, rest)) = eligible.split_first() else {
                // Unreachable (eligible non-empty checked above), but settle
                // defensively rather than parking on an empty queue.
                return settle(
                    state,
                    controller,
                    *opening,
                    &winner_effect,
                    resolved_target,
                    controller,
                    ability.source_id,
                    events,
                );
            };
            state.waiting_for = WaitingFor::AuctionBid {
                player: *first,
                current_high_bid: *opening,
                high_bidder: Some(controller),
                eligible: eligible.clone(),
                remaining_in_round: rest.to_vec(),
                passes_in_a_row: 0,
                winner_effect,
                target: resolved_target,
                controller,
                source_id: ability.source_id,
            };
        }
    }

    Ok(())
}

/// Return the controller of `id` if it is a spell (or other object) currently
/// on the stack; otherwise `None`. Used to detect the Mages' Contest two-bidder
/// case (`you and target spell's controller`).
fn stack_spell_controller(state: &GameState, id: ObjectId) -> Option<PlayerId> {
    state
        .stack
        .iter()
        .find(|entry| entry.id == id)
        .map(|entry| entry.controller)
}

/// CR 119.3 + CR 608.2c: Settle the auction. The high bidder loses life equal
/// to the high bid, then `winner_effect` resolves once bound to the high
/// bidder. For Mages' Contest the Counter payoff is gated on the high bidder
/// being the caster ("if you win the bidding, counter that spell"); life loss
/// applies to the high bidder regardless.
#[allow(clippy::too_many_arguments)]
pub(crate) fn settle(
    state: &mut GameState,
    high_bidder: PlayerId,
    high_bid: u32,
    winner_effect: &crate::types::ability::AbilityDefinition,
    target: Option<ObjectId>,
    caster: PlayerId,
    source_id: ObjectId,
    events: &mut Vec<GameEvent>,
) -> Result<(), EffectError> {
    bid_amount_in_range(high_bid).map_err(|msg| EffectError::InvalidParam(msg.into()))?;

    // CR 119.3: The high bidder loses life equal to the high bid.
    if high_bid > 0 {
        let lose = lose_life_ability(high_bidder, high_bid, source_id);
        // Leaf payoff — call `resolve_effect` directly so settlement failures
        // propagate (`resolve_ability_chain` swallows handler errors in its
        // single-iteration loop).
        crate::game::effects::resolve_effect(state, &lose, events)?;
    }

    // CR 608.2c: Resolve the payoff for the high bidder. The Mages' Contest
    // Counter is gated on the caster winning ("if you win the bidding").
    let is_counter = matches!(*winner_effect.effect, Effect::Counter { .. });
    if is_counter && high_bidder != caster {
        // "If you win the bidding, counter that spell." The caster did not win,
        // so the spell is NOT countered. Life loss already applied above.
        events.push(GameEvent::EffectResolved {
            kind: EffectKind::AuctionBid,
            source_id,
        });
        return Ok(());
    }

    let winner_ability = resolved_winner(winner_effect, source_id, high_bidder, target);
    crate::game::effects::resolve_effect(state, &winner_ability, events)?;

    events.push(GameEvent::EffectResolved {
        kind: EffectKind::AuctionBid,
        source_id,
    });
    Ok(())
}

/// Build a `LoseLife { amount }` ability bound to `player` as controller. With
/// `target: None`, `resolve_life_loss_target` falls back to the controller, so
/// the high bidder loses the life.
fn lose_life_ability(player: PlayerId, amount: u32, source_id: ObjectId) -> ResolvedAbility {
    let loss = i32::try_from(amount).expect("auction bids are range-checked before settlement");
    let def = crate::types::ability::AbilityDefinition::new(
        crate::types::ability::AbilityKind::Spell,
        Effect::LoseLife {
            amount: crate::types::ability::QuantityExpr::Fixed { value: loss },
            target: None,
        },
    );
    resolved_from_def(&def, source_id, player, None)
}

/// CR 608.2c: Resolve the winner payoff with `controller = high_bidder`. For a
/// targeted payoff (GainControl over the creature, Counter on the stack spell)
/// the resolved target object is injected into `targets` so the resolver acts
/// on the auctioned object.
fn resolved_winner(
    def: &crate::types::ability::AbilityDefinition,
    source_id: ObjectId,
    high_bidder: PlayerId,
    target: Option<ObjectId>,
) -> ResolvedAbility {
    let targets = match target {
        Some(id) if def.effect.target_filter().is_some() => vec![TargetRef::Object(id)],
        _ => Vec::new(),
    };
    resolved_from_def(def, source_id, high_bidder, Some(targets))
}

/// Convert a stored `AbilityDefinition` into a `ResolvedAbility` carrying the
/// given source/controller, mirroring `vote::resolved_from_def` (the GAP1
/// winner-binding shape: `original_controller: None`, `scoped_player: None`, so
/// GainControl/Draw resolve FOR `controller`). `targets` defaults to empty.
fn resolved_from_def(
    def: &crate::types::ability::AbilityDefinition,
    source_id: ObjectId,
    controller: PlayerId,
    targets: Option<Vec<TargetRef>>,
) -> ResolvedAbility {
    ResolvedAbility {
        effect: (*def.effect).clone(),
        targets: targets.unwrap_or_default(),
        source_id,
        source_incarnation: None,
        controller,
        original_controller: None,
        scoped_player: None,
        target_chooser: None,
        kind: def.kind,
        sub_ability: def
            .sub_ability
            .as_ref()
            .map(|sub| Box::new(resolved_from_def(sub, source_id, controller, None))),
        else_ability: None,
        duration: def.duration.clone(),
        condition: def.condition.clone(),
        context: Default::default(),
        optional_targeting: def.optional_targeting,
        optional: def.optional,
        optional_for: None,
        multi_target: None,
        target_constraints: Vec::new(),
        target_choice_timing: def.target_choice_timing,
        description: def.description.clone(),
        repeat_for: None,
        min_x_value: def.min_x_value,
        cant_be_copied: def.cant_be_copied,
        copy_count_status: crate::types::ability::CopyCountStatus::Pending,
        forward_result: def.forward_result,
        unless_pay: None,
        distribution: None,
        player_scope: None,
        starting_with: def.starting_with.clone(),
        chosen_x: None,
        cost_paid_object: None,
        effect_context_object: None,
        ability_index: None,
        may_trigger_origin: None,
        target_selection_mode: def.target_selection_mode,
        chosen_players: Vec::new(),
        repeat_until: None,
        sub_link: def.sub_link,
        modal: def.modal.clone(),
        mode_abilities: def.mode_abilities.clone(),
        dig_found_nothing_for_parent_target: false,
    }
}
