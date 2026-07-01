//! CR 119.3 + CR 101.4 + CR 608.2: End-to-end runtime tests for the open-bid
//! life-auction subsystem (`Effect::AuctionBid` — Illicit Auction, Pain's
//! Reward, Mages' Contest). These drive REAL `GameAction::SubmitBid` actions
//! through the public `apply` API so the full park → bid loop → settle →
//! winner-effect path executes (the dead-flip guard: a settle that binds the
//! winner via the same `resolved_from_def` shape the vote tally uses).
//!
//! Each positive test has a discriminating revert-fail assertion: if the
//! winner-binding / life-loss path regressed, the asserted control-change /
//! draw / counter would not be present.

use engine::game::effects::auction;
use engine::game::engine::apply;
use engine::game::zones::create_object;
use engine::types::ability::{
    AbilityDefinition, AbilityKind, AuctionOpening, Effect, QuantityExpr, ResolvedAbility,
    TargetFilter, TargetRef, TypedFilter, VoterScope,
};
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::game_state::{
    CastingVariant, GameState, StackEntry, StackEntryKind, WaitingFor,
};
use engine::types::identifiers::{CardId, ObjectId};
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

/// Build a `ResolvedAbility` for an `Effect::AuctionBid`, with `targets`
/// pre-bound (the resolved creature / stack spell).
fn auction_ability(
    effect: Effect,
    source_id: ObjectId,
    controller: PlayerId,
    targets: Vec<TargetRef>,
) -> ResolvedAbility {
    ResolvedAbility {
        effect,
        targets,
        source_id,
        source_incarnation: None,
        controller,
        original_controller: None,
        scoped_player: None,
        target_chooser: None,
        kind: AbilityKind::Spell,
        sub_ability: None,
        else_ability: None,
        duration: None,
        condition: None,
        context: Default::default(),
        optional_targeting: false,
        optional: false,
        optional_for: None,
        multi_target: None,
        target_constraints: Vec::new(),
        target_choice_timing: engine::types::ability::TargetChoiceTiming::Stack,
        description: None,
        repeat_for: None,
        min_x_value: 0,
        cant_be_copied: false,
        copy_count_status: engine::types::ability::CopyCountStatus::Pending,
        forward_result: false,
        unless_pay: None,
        distribution: None,
        player_scope: None,
        starting_with: None,
        chosen_x: None,
        cost_paid_object: None,
        effect_context_object: None,
        ability_index: None,
        may_trigger_origin: None,
        target_selection_mode: engine::types::ability::TargetSelectionMode::Chosen,
        chosen_players: Vec::new(),
        repeat_until: None,
        sub_link: engine::types::ability::SubAbilityLink::ContinuationStep,
        modal: None,
        mode_abilities: vec![],
    }
}

fn gain_control_def() -> Box<AbilityDefinition> {
    Box::new(AbilityDefinition::new(
        AbilityKind::Spell,
        Effect::GainControl {
            target: TargetFilter::Typed(TypedFilter::creature()),
        },
    ))
}

fn draw_four_def() -> Box<AbilityDefinition> {
    Box::new(AbilityDefinition::new(
        AbilityKind::Spell,
        Effect::Draw {
            count: QuantityExpr::Fixed { value: 4 },
            target: TargetFilter::Controller,
        },
    ))
}

fn counter_def() -> Box<AbilityDefinition> {
    Box::new(AbilityDefinition::new(
        AbilityKind::Spell,
        Effect::Counter {
            target: TargetFilter::StackSpell,
            source_rider: None,
            countered_spell_zone: None,
        },
    ))
}

fn life(state: &GameState, p: PlayerId) -> i32 {
    state.players.iter().find(|pl| pl.id == p).unwrap().life
}

/// Test 1 — Illicit Auction: P1 casts targeting P2's creature; P1 bids 5, P2
/// passes → P1 loses 5 life AND gains control of the creature.
#[test]
fn illicit_auction_high_bidder_loses_life_and_gains_control() {
    let mut state = GameState::new_two_player(42);
    let p1 = state.players[0].id;
    let p2 = state.players[1].id;

    // P2's creature on the battlefield.
    let creature = create_object(
        &mut state,
        CardId(10),
        p2,
        "Bear".to_string(),
        Zone::Battlefield,
    );
    state
        .objects
        .get_mut(&creature)
        .unwrap()
        .card_types
        .core_types
        .push(CoreType::Creature);

    let source = ObjectId(900);
    let p1_life_before = life(&state, p1);

    let ability = auction_ability(
        Effect::AuctionBid {
            opening_bid: AuctionOpening::Fixed(0),
            voter_scope: VoterScope::AllPlayers,
            winner_effect: gain_control_def(),
            target: TargetFilter::Typed(TypedFilter::creature()),
        },
        source,
        p1,
        vec![TargetRef::Object(creature)],
    );

    let mut events = Vec::new();
    auction::resolve(&mut state, &ability, &mut events).expect("auction parks");

    // P1 (controller) acts first as the standing high bidder at 0.
    assert!(matches!(state.waiting_for, WaitingFor::AuctionBid { player, .. } if player == p1));
    apply(&mut state, p1, GameAction::SubmitBid { amount: 5 }).expect("P1 bids 5");
    // P2 passes (any amount <= high bid).
    apply(&mut state, p2, GameAction::SubmitBid { amount: 0 }).expect("P2 passes");

    // P1 lost 5 life.
    assert_eq!(life(&state, p1), p1_life_before - 5, "P1 must lose 5 life");
    // P1 gained control of the creature (control-change present).
    assert_eq!(
        state.objects.get(&creature).unwrap().controller,
        p1,
        "P1 must control the creature after winning the auction"
    );
}

/// Test 2 — Pain's Reward: player-chosen opening. P1 opens at 3, P2 passes →
/// P1 loses 3 life AND draws four cards.
#[test]
fn pains_reward_opening_bid_loses_life_and_draws_four() {
    let mut state = GameState::new_two_player(7);
    let p1 = state.players[0].id;
    let p2 = state.players[1].id;

    // Stock P1's library so the draw of four has cards.
    for i in 0..6 {
        let c = create_object(
            &mut state,
            CardId(100 + i),
            p1,
            format!("Card{i}"),
            Zone::Library,
        );
        // create_object already adds to the library zone via add_to_zone.
        let _ = c;
    }
    let hand_before = state.players[0].hand.len();
    let p1_life_before = life(&state, p1);
    let source = ObjectId(901);

    let ability = auction_ability(
        Effect::AuctionBid {
            opening_bid: AuctionOpening::PlayerChosen,
            voter_scope: VoterScope::AllPlayers,
            winner_effect: draw_four_def(),
            target: TargetFilter::None,
        },
        source,
        p1,
        vec![],
    );

    let mut events = Vec::new();
    auction::resolve(&mut state, &ability, &mut events).expect("auction parks");

    // Opening phase: P1 sets the opening bid to 3. The opening phase is
    // exactly `high_bidder: None` (no `opening_phase` flag).
    assert!(matches!(
        state.waiting_for,
        WaitingFor::AuctionBid { player, high_bidder: None, .. } if player == p1
    ));
    apply(&mut state, p1, GameAction::SubmitBid { amount: 3 }).expect("P1 opens at 3");
    // P2 passes.
    apply(&mut state, p2, GameAction::SubmitBid { amount: 0 }).expect("P2 passes");

    assert_eq!(life(&state, p1), p1_life_before - 3, "P1 must lose 3 life");
    assert_eq!(
        state.players[0].hand.len(),
        hand_before + 4,
        "P1 must draw four cards"
    );
}

/// Push a dummy spell on the stack controlled by `controller`, returning its id.
fn push_stack_spell(state: &mut GameState, controller: PlayerId, card: CardId) -> ObjectId {
    let id = create_object(
        state,
        card,
        controller,
        "Stack Spell".to_string(),
        Zone::Stack,
    );
    state.stack.push_back(StackEntry {
        id,
        source_id: id,
        controller,
        kind: StackEntryKind::Spell {
            card_id: card,
            ability: None,
            casting_variant: CastingVariant::Normal,
            actual_mana_spent: 0,
        },
    });
    id
}

/// Test 3 (positive) — Mages' Contest: P1 (caster) wins → spell countered.
#[test]
fn mages_contest_caster_wins_counters_the_spell() {
    let mut state = GameState::new_two_player(11);
    let p1 = state.players[0].id;
    let p2 = state.players[1].id;

    let spell = push_stack_spell(&mut state, p2, CardId(50));
    let source = ObjectId(902);
    let p1_life_before = life(&state, p1);

    let ability = auction_ability(
        Effect::AuctionBid {
            opening_bid: AuctionOpening::Fixed(1),
            voter_scope: VoterScope::AllPlayers,
            winner_effect: counter_def(),
            target: TargetFilter::StackSpell,
        },
        source,
        p1,
        vec![TargetRef::Object(spell)],
    );

    let mut events = Vec::new();
    auction::resolve(&mut state, &ability, &mut events).expect("auction parks");

    // Two-bidder queue: P1 (caster) first at the opening of 1.
    apply(&mut state, p1, GameAction::SubmitBid { amount: 4 }).expect("P1 bids 4");
    apply(&mut state, p2, GameAction::SubmitBid { amount: 1 }).expect("P2 passes");

    assert_eq!(
        life(&state, p1),
        p1_life_before - 4,
        "P1 (caster) loses 4 life"
    );
    // The spell was countered → it left the stack (to graveyard).
    assert!(
        !state.stack.iter().any(|e| e.id == spell),
        "the targeted spell must be countered (off the stack)"
    );
}

/// Test 3 (negative / you-gate) — Mages' Contest: P2 outbids P1, so the caster
/// does NOT win → spell is NOT countered, P2 loses the life.
#[test]
fn mages_contest_opponent_wins_does_not_counter() {
    let mut state = GameState::new_two_player(13);
    let p1 = state.players[0].id;
    let p2 = state.players[1].id;

    let spell = push_stack_spell(&mut state, p2, CardId(51));
    let source = ObjectId(903);
    let p2_life_before = life(&state, p2);

    let ability = auction_ability(
        Effect::AuctionBid {
            opening_bid: AuctionOpening::Fixed(1),
            voter_scope: VoterScope::AllPlayers,
            winner_effect: counter_def(),
            target: TargetFilter::StackSpell,
        },
        source,
        p1,
        vec![TargetRef::Object(spell)],
    );

    let mut events = Vec::new();
    auction::resolve(&mut state, &ability, &mut events).expect("auction parks");

    // P1 declines to raise the opening (pass), P2 outbids to 5, P1 passes.
    apply(&mut state, p1, GameAction::SubmitBid { amount: 1 }).expect("P1 passes opening");
    apply(&mut state, p2, GameAction::SubmitBid { amount: 5 }).expect("P2 bids 5");
    apply(&mut state, p1, GameAction::SubmitBid { amount: 0 }).expect("P1 passes");

    assert_eq!(
        life(&state, p2),
        p2_life_before - 5,
        "P2 (winner) loses 5 life"
    );
    // You-gate: caster did NOT win, so the spell stays on the stack.
    assert!(
        state.stack.iter().any(|e| e.id == spell),
        "the spell must NOT be countered when the caster loses the bidding"
    );
}

/// Test 4 — AI safe default: `get_legal_actions` on an AuctionBid prompt
/// returns the safe pass (SubmitBid at the current high bid; never bids life).
#[test]
fn ai_legal_actions_on_auction_returns_safe_pass() {
    let mut state = GameState::new_two_player(99);
    let p1 = state.players[0].id;
    let p2 = state.players[1].id;
    let source = ObjectId(904);

    state.waiting_for = WaitingFor::AuctionBid {
        player: p2,
        current_high_bid: 3,
        high_bidder: Some(p1),
        eligible: vec![p1, p2],
        remaining_in_round: vec![],
        passes_in_a_row: 0,
        winner_effect: gain_control_def(),
        target: None,
        controller: p1,
        source_id: source,
    };

    let actions = engine::ai_support::legal_actions(&state);
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, GameAction::SubmitBid { amount } if *amount == 3)),
        "AI must offer the safe pass (SubmitBid at the current high bid), got {actions:?}"
    );
    // The safe default must NEVER bid above the current high bid (no life burn).
    assert!(
        !actions
            .iter()
            .any(|a| matches!(a, GameAction::SubmitBid { amount } if *amount > 3)),
        "AI safe default must not bid life above the high bid"
    );
}

/// CR 119.3: an opening bid above the bidder's current life is legal — bids
/// cause life loss at settlement, not as a payable cost. P1 (Pain's Reward) has
/// only 2 life and opens at 5 → accepted, life unchanged until settlement.
#[test]
fn opening_bid_above_life_is_allowed() {
    let mut state = GameState::new_two_player(7);
    let p1 = state.players[0].id;
    let p2 = state.players[1].id;
    state.players[0].life = 2;
    let source = ObjectId(905);

    let ability = auction_ability(
        Effect::AuctionBid {
            opening_bid: AuctionOpening::PlayerChosen,
            voter_scope: VoterScope::AllPlayers,
            winner_effect: draw_four_def(),
            target: TargetFilter::None,
        },
        source,
        p1,
        vec![],
    );

    let mut events = Vec::new();
    auction::resolve(&mut state, &ability, &mut events).expect("auction parks");

    apply(&mut state, p1, GameAction::SubmitBid { amount: 5 })
        .expect("an opening bid above current life must be legal (CR 119.3)");
    assert_eq!(life(&state, p1), 2, "life is not lost until settlement");
    assert!(matches!(
        state.waiting_for,
        WaitingFor::AuctionBid {
            player,
            current_high_bid: 5,
            high_bidder: Some(winner),
            ..
        } if player == p2 && winner == p1
    ));
}

/// CR 119.3: a topping bid above the bidder's current life is legal. P2 has
/// only 2 life and tops P1's standing bid of 3 with 6 → accepted, life unchanged.
#[test]
fn topping_bid_above_life_is_allowed() {
    let mut state = GameState::new_two_player(11);
    let p1 = state.players[0].id;
    let p2 = state.players[1].id;
    state.players[1].life = 2;
    let source = ObjectId(906);

    state.waiting_for = WaitingFor::AuctionBid {
        player: p2,
        current_high_bid: 3,
        high_bidder: Some(p1),
        eligible: vec![p1, p2],
        remaining_in_round: vec![],
        passes_in_a_row: 0,
        winner_effect: gain_control_def(),
        target: None,
        controller: p1,
        source_id: source,
    };

    apply(&mut state, p2, GameAction::SubmitBid { amount: 6 })
        .expect("a topping bid above current life must be legal (CR 119.3)");
    assert_eq!(life(&state, p2), 2, "life is not lost until settlement");
    assert!(matches!(
        state.waiting_for,
        WaitingFor::AuctionBid {
            current_high_bid: 6,
            high_bidder: Some(winner),
            ..
        } if winner == p2
    ));
}

/// Revert-fail guard: a settlement failure must propagate and preserve the
/// auction prompt instead of clearing it via `finish_with_continuation`.
#[test]
fn settlement_failure_preserves_auction_prompt() {
    let mut state = GameState::new_two_player(42);
    let p1 = state.players[0].id;
    let p2 = state.players[1].id;

    let creature = create_object(
        &mut state,
        CardId(10),
        p2,
        "Bear".to_string(),
        Zone::Battlefield,
    );
    state
        .objects
        .get_mut(&creature)
        .unwrap()
        .card_types
        .core_types
        .push(CoreType::Creature);

    let source = ObjectId(907);
    let ability = auction_ability(
        Effect::AuctionBid {
            opening_bid: AuctionOpening::Fixed(0),
            voter_scope: VoterScope::AllPlayers,
            winner_effect: gain_control_def(),
            target: TargetFilter::Typed(TypedFilter::creature()),
        },
        source,
        p1,
        vec![TargetRef::Object(creature)],
    );

    let mut events = Vec::new();
    auction::resolve(&mut state, &ability, &mut events).expect("auction parks");

    // P1 passes the opening bid of 0; P2 is next to act.
    apply(&mut state, p1, GameAction::SubmitBid { amount: 0 }).expect("P1 passes opening");

    // The auction target is gone before settlement — GainControl must fail when
    // the final pass triggers `auction::settle`.
    state.battlefield.retain(|id| *id != creature);
    state.objects.remove(&creature);
    let parked = state.waiting_for.clone();

    let result = apply(&mut state, p2, GameAction::SubmitBid { amount: 0 });
    assert!(
        result.is_err(),
        "settlement failure must propagate to the caller"
    );
    assert_eq!(
        state.waiting_for, parked,
        "failed settlement must not clear the auction prompt"
    );
}
